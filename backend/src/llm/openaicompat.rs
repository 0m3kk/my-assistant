//! OpenAI Chat Completions compatible streaming adapter.
//!
//! Chat uses ONLY `POST {base_url}/chat/completions` — no `/responses`, no
//! `stream_options` (some compatible servers reject unknown fields), so usage
//! is parsed from the final chunk when present and simply absent otherwise.
//! `GET {base_url}/models` is used only for admin-triggered model discovery.

use std::collections::BTreeMap;

use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{
    DiscoveredModel, LlmMessage, Provider, ProviderCredentials, StreamCallbacks, StreamError,
    StreamOutcome, ToolCallReq, ToolDef,
};

pub struct OpenAiCompatProvider {
    http: reqwest::Client,
}

impl OpenAiCompatProvider {
    pub fn new(http: reqwest::Client) -> Self {
        Self { http }
    }

    fn encode_messages(messages: &[LlmMessage]) -> Vec<Value> {
        messages
            .iter()
            .map(|m| match m {
                LlmMessage::System { content } => json!({"role": "system", "content": content}),
                LlmMessage::User { content, images } => {
                    if images.is_empty() {
                        json!({"role": "user", "content": content})
                    } else {
                        // Multimodal content: text block first, then images.
                        let mut blocks = vec![json!({"type": "text", "text": content})];
                        blocks.extend(images.iter().map(|url| {
                            json!({"type": "image_url", "image_url": {"url": url}})
                        }));
                        json!({"role": "user", "content": blocks})
                    }
                }
                LlmMessage::Assistant { content, tool_calls } => {
                    let mut msg = json!({"role": "assistant"});
                    if let Some(c) = content {
                        msg["content"] = json!(c);
                    } else {
                        msg["content"] = Value::Null;
                    }
                    if !tool_calls.is_empty() {
                        msg["tool_calls"] = json!(
                            tool_calls.iter().map(|tc| json!({
                                "id": tc.id,
                                "type": "function",
                                "function": {"name": tc.name, "arguments": tc.arguments.to_string()},
                            })).collect::<Vec<_>>()
                        );
                    }
                    msg
                }
                LlmMessage::ToolResult { tool_call_id, content } => {
                    json!({"role": "tool", "tool_call_id": tool_call_id, "content": content})
                }
            })
            .collect()
    }

    fn encode_tools(tools: &[ToolDef]) -> Option<Value> {
        if tools.is_empty() {
            return None;
        }
        Some(json!(
            tools
                .iter()
                .map(|t| json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters_schema,
                    },
                }))
                .collect::<Vec<_>>()
        ))
    }

    /// POST a chat-completions body (auth header applied when a key exists).
    async fn send_chat(
        &self,
        url: &str,
        body: &Value,
        credentials: &ProviderCredentials,
    ) -> super::anyhow_like::Result<reqwest::Response> {
        let mut req = self.http.post(url).json(body);
        if let Some(key) = &credentials.api_key {
            req = req.bearer_auth(key);
        }
        req.send()
            .await
            .map_err(|e| StreamError::Network(e.to_string()))
    }

    /// Parse a `/models` payload. Accepts both the OpenAI envelope
    /// (`{"data": [...]}`) and a bare array (some compatible servers).
    fn parse_models(payload: &Value) -> Vec<DiscoveredModel> {
        let entries = payload
            .get("data")
            .and_then(Value::as_array)
            .or_else(|| payload.as_array());
        let Some(entries) = entries else {
            return Vec::new();
        };
        entries
            .iter()
            .filter_map(|entry| {
                let api_name = entry.get("id").and_then(Value::as_str)?.trim().to_string();
                if api_name.is_empty() {
                    return None;
                }
                let display_name = entry
                    .get("name")
                    .or_else(|| entry.get("display_name"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or(&api_name)
                    .to_string();
                // OpenRouter uses `context_length`; others `context_window`
                // or `max_context_length` — accept whichever is present.
                let context_window = ["context_length", "context_window", "max_context_length"]
                    .iter()
                    .find_map(|key| {
                        entry
                            .get(*key)
                            .and_then(Value::as_i64)
                            .filter(|v| *v > 0 && *v <= i32::MAX as i64)
                            .map(|v| v as i32)
                    });
                Some(DiscoveredModel {
                    api_name,
                    display_name,
                    context_window,
                })
            })
            .collect()
    }
}

#[async_trait]
impl Provider for OpenAiCompatProvider {
    fn id(&self) -> &'static str {
        "openaicompat"
    }

    async fn list_models(
        &self,
        credentials: &ProviderCredentials,
    ) -> super::anyhow_like::Result<Vec<DiscoveredModel>> {
        let url = format!("{}/models", credentials.base_url.trim_end_matches('/'));
        let mut req = self.http.get(&url);
        if let Some(key) = &credentials.api_key {
            req = req.bearer_auth(key);
        }
        let response = req
            .send()
            .await
            .map_err(|e| StreamError::Network(e.to_string()))?;

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            let body: String = body.chars().take(2000).collect();
            return Err(StreamError::Upstream {
                status: status.as_u16(),
                body,
            });
        }
        let payload: Value = serde_json::from_str(&body)
            .map_err(|e| StreamError::Malformed(format!("bad models json: {e}")))?;
        Ok(Self::parse_models(&payload))
    }

    #[allow(clippy::too_many_arguments)]
    async fn stream_chat(
        &self,
        _run_id: Uuid,
        model: &str,
        messages: &[LlmMessage],
        tools: &[ToolDef],
        temperature: Option<f32>,
        max_tokens: Option<i32>,
        reasoning_effort: Option<&str>,
        credentials: &ProviderCredentials,
        callbacks: &mut dyn StreamCallbacks,
    ) -> super::anyhow_like::Result<StreamOutcome> {
        let url = format!(
            "{}/chat/completions",
            credentials.base_url.trim_end_matches('/')
        );
        let mut body = json!({
            "model": model,
            "messages": Self::encode_messages(messages),
            "stream": true,
        });
        if let Some(tools) = Self::encode_tools(tools) {
            body["tools"] = tools;
            body["tool_choice"] = json!("auto");
        }
        if let Some(t) = temperature {
            body["temperature"] = json!(t);
        }
        if let Some(mt) = max_tokens {
            if mt > 0 {
                body["max_tokens"] = json!(mt);
            }
        }
        // Only sent when configured — some compatible servers reject unknown fields.
        if let Some(effort) = reasoning_effort {
            body["reasoning_effort"] = json!(effort);
        }

        // Usage (context meter) only arrives for streamed responses when asked
        // for explicitly. Servers that don't understand the field are retried
        // once without it below.
        body["stream_options"] = json!({"include_usage": true});

        let response = self.send_chat(&url, &body, credentials).await?;

        let status = response.status();
        let mut response = response;
        if !status.is_success() {
            let code = status.as_u16();
            let text = response.text().await.unwrap_or_default();
            if code == 400 || code == 422 {
                // Retry without `stream_options` — the streaming reply still
                // works, only the usage numbers are unavailable.
                if let Some(obj) = body.as_object_mut() {
                    obj.remove("stream_options");
                }
                let retry = self.send_chat(&url, &body, credentials).await?;
                if retry.status().is_success() {
                    response = retry;
                } else {
                    let code = retry.status().as_u16();
                    let body = retry.text().await.unwrap_or_default();
                    let body: String = body.chars().take(2000).collect();
                    return Err(StreamError::Upstream { status: code, body });
                }
            } else {
                // Truncate large error bodies before they reach logs or the client.
                let body: String = text.chars().take(2000).collect();
                return Err(StreamError::Upstream { status: code, body });
            }
        }

        let byte_stream = response.bytes_stream();
        let mut event_stream = byte_stream.eventsource();

        let mut outcome = StreamOutcome::default();
        let mut partial: BTreeMap<u32, PartialToolCall> = BTreeMap::new();

        while let Some(event) = event_stream.next().await {
            let event = event.map_err(|e| StreamError::Malformed(format!("sse error: {e}")))?;
            let data = event.data.trim();
            if data.is_empty() || data == "[DONE]" {
                continue;
            }
            let payload: Value = serde_json::from_str(data)
                .map_err(|e| StreamError::Malformed(format!("bad chunk json: {e}")))?;

            if let Some(err) = payload.get("error") {
                let msg = err
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown provider error");
                return Err(StreamError::Upstream {
                    status: 200,
                    body: msg.to_string(),
                });
            }

            if let Some(usage) = payload.get("usage") {
                outcome.usage.prompt_tokens = usage.get("prompt_tokens").and_then(Value::as_i64);
                outcome.usage.completion_tokens =
                    usage.get("completion_tokens").and_then(Value::as_i64);
            }

            let Some(choices) = payload.get("choices").and_then(Value::as_array) else {
                continue;
            };
            for choice in choices {
                if let Some(delta) = choice.get("delta") {
                    if let Some(content) = delta.get("content").and_then(Value::as_str) {
                        if !content.is_empty() {
                            outcome.text.push_str(content);
                            callbacks.on_text_delta(content);
                        }
                    }
                    // DeepSeek-style reasoning extensions; absent on most servers.
                    let reasoning = delta
                        .get("reasoning_content")
                        .or_else(|| delta.get("reasoning"))
                        .and_then(Value::as_str);
                    if let Some(r) = reasoning {
                        if !r.is_empty() {
                            outcome.reasoning.push_str(r);
                            callbacks.on_reasoning_delta(r);
                        }
                    }
                    if let Some(tcs) = delta.get("tool_calls").and_then(Value::as_array) {
                        for tc in tcs {
                            let index = tc.get("index").and_then(Value::as_u64).unwrap_or(0) as u32;
                            let entry = partial.entry(index).or_default();
                            if let Some(id) = tc.get("id").and_then(Value::as_str) {
                                entry.id.push_str(id);
                            }
                            if let Some(name) = tc
                                .get("function")
                                .and_then(|f| f.get("name"))
                                .and_then(Value::as_str)
                            {
                                entry.name.push_str(name);
                            }
                            if let Some(args) = tc
                                .get("function")
                                .and_then(|f| f.get("arguments"))
                                .and_then(Value::as_str)
                            {
                                entry.arguments.push_str(args);
                            }
                        }
                    }
                }
                if let Some(finish) = choice.get("finish_reason").and_then(Value::as_str) {
                    if !finish.is_empty() {
                        outcome.finish_reason = Some(finish.to_string());
                    }
                }
            }
        }

        // Assemble complete tool calls in index order.
        for (_, p) in partial {
            if p.name.is_empty() {
                continue;
            }
            let arguments: Value = if p.arguments.trim().is_empty() {
                json!({})
            } else {
                serde_json::from_str(p.arguments.trim())
                    .map_err(|e| StreamError::Malformed(format!("tool arguments json: {e}")))?
            };
            let call = ToolCallReq {
                id: p.id,
                name: p.name,
                arguments,
            };
            callbacks.on_tool_call(&call);
            outcome.tool_calls.push(call);
        }

        Ok(outcome)
    }
}

#[derive(Default)]
struct PartialToolCall {
    id: String,
    name: String,
    arguments: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn sse_body(chunks: &[Value]) -> String {
        let mut s = String::new();
        for c in chunks {
            s.push_str(&format!("data: {}\n\n", c));
        }
        s.push_str("data: [DONE]\n\n");
        s
    }

    #[test]
    fn user_message_encodes_as_plain_text_without_images() {
        let wire = OpenAiCompatProvider::encode_messages(&[LlmMessage::User {
            content: "hi".into(),
            images: vec![],
        }]);
        assert_eq!(wire[0]["content"], json!("hi"));
    }

    #[test]
    fn user_message_encodes_multimodal_blocks_with_images() {
        let wire = OpenAiCompatProvider::encode_messages(&[LlmMessage::User {
            content: "what is this?".into(),
            images: vec!["data:image/png;base64,AAAA".into()],
        }]);
        assert!(wire[0]["content"].is_array());
        assert_eq!(wire[0]["content"][0]["type"], json!("text"));
        assert_eq!(
            wire[0]["content"][1],
            json!({"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}})
        );
    }

    #[test]
    fn parses_models_from_openai_envelope() {
        let payload = json!({
            "object": "list",
            "data": [
                {"id": "gpt-4o", "object": "model"},
                {"id": "glm-4.6", "name": "GLM 4.6", "context_length": 200000}
            ]
        });
        let models = OpenAiCompatProvider::parse_models(&payload);
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].api_name, "gpt-4o");
        assert_eq!(models[0].display_name, "gpt-4o"); // falls back to api_name
        assert_eq!(models[0].context_window, None);
        assert_eq!(models[1].display_name, "GLM 4.6");
        assert_eq!(models[1].context_window, Some(200_000));
    }

    #[test]
    fn parses_models_from_bare_array_and_skips_junk() {
        let payload = json!([
            {"id": "a"},
            {"id": "   "},
            {"name": "no id"},
            {"id": "b", "context_window": 8192}
        ]);
        let models = OpenAiCompatProvider::parse_models(&payload);
        let names: Vec<_> = models.iter().map(|m| m.api_name.as_str()).collect();
        assert_eq!(names, vec!["a", "b"]);
        assert_eq!(models[1].context_window, Some(8192));
    }

    #[test]
    fn rejects_malformed_models_payload() {
        assert!(OpenAiCompatProvider::parse_models(&json!({"error": "nope"})).is_empty());
    }

    #[tokio::test]
    async fn lists_models_from_upstream() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/models"))
            .and(wiremock::matchers::header("authorization", "Bearer k"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
                "data": [{"id": "m1", "context_length": 4096}]
            })))
            .mount(&server)
            .await;

        let provider = OpenAiCompatProvider::new(reqwest::Client::new());
        let models = provider
            .list_models(&ProviderCredentials {
                base_url: server.uri(),
                api_key: Some("k".into()),
            })
            .await
            .unwrap();

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].api_name, "m1");
        assert_eq!(models[0].context_window, Some(4096));
    }

    #[tokio::test]
    async fn discovery_surfaces_upstream_error() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(wiremock::ResponseTemplate::new(403).set_body_string(r#"{"error":"no"}"#))
            .mount(&server)
            .await;

        let provider = OpenAiCompatProvider::new(reqwest::Client::new());
        let err = provider
            .list_models(&ProviderCredentials {
                base_url: server.uri(),
                api_key: None,
            })
            .await
            .unwrap_err();

        match err {
            StreamError::Upstream { status, .. } => assert_eq!(status, 403),
            other => panic!("expected upstream error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn requests_stream_usage_and_retries_without_it_on_400() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::body_partial_json(
                json!({"stream_options": {"include_usage": true}}),
            ))
            .respond_with(
                wiremock::ResponseTemplate::new(400)
                    .set_body_string(r#"{"error":{"message":"unknown field stream_options"}}"#),
            )
            .with_priority(1)
            .mount(&server)
            .await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_string(sse_body(&[json!(
                        {"choices":[{"delta":{"content":"ok"},"finish_reason":"stop"}]}
                    )]))
                    .append_header("content-type", "text/event-stream"),
            )
            .with_priority(2)
            .mount(&server)
            .await;

        let provider = OpenAiCompatProvider::new(reqwest::Client::new());
        let mut cb = Recorder::default();
        let outcome = provider
            .stream_chat(
                Uuid::new_v4(),
                "m",
                &[],
                &[],
                None,
                None,
                None,
                &ProviderCredentials {
                    base_url: server.uri(),
                    api_key: None,
                },
                &mut cb,
            )
            .await
            .unwrap();

        assert_eq!(outcome.text, "ok");
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 2, "expected the 400 to be retried once");
        let first: Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(first["stream_options"], json!({"include_usage": true}));
        let second: Value = serde_json::from_slice(&requests[1].body).unwrap();
        assert!(second.get("stream_options").is_none());
    }

    #[tokio::test]
    async fn does_not_retry_non_400_errors() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .respond_with(
                wiremock::ResponseTemplate::new(401).set_body_string(r#"{"error":"bad key"}"#),
            )
            .mount(&server)
            .await;

        let provider = OpenAiCompatProvider::new(reqwest::Client::new());
        let mut cb = Recorder::default();
        let err = provider
            .stream_chat(
                Uuid::new_v4(),
                "m",
                &[],
                &[],
                None,
                None,
                None,
                &ProviderCredentials {
                    base_url: server.uri(),
                    api_key: Some("k".into()),
                },
                &mut cb,
            )
            .await
            .unwrap_err();

        assert!(matches!(err, StreamError::Upstream { status: 401, .. }));
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }

    #[derive(Clone, Default)]
    struct Recorder {
        text: Arc<Mutex<String>>,
        tools: Arc<Mutex<Vec<ToolCallReq>>>,
    }

    impl StreamCallbacks for Recorder {
        fn on_text_delta(&mut self, delta: &str) {
            self.text.lock().unwrap().push_str(delta);
        }
        fn on_tool_call(&mut self, call: &ToolCallReq) {
            self.tools.lock().unwrap().push(call.clone());
        }
    }

    #[tokio::test]
    async fn streams_text_and_usage() {
        let chunks = vec![
            json!({"id":"1","choices":[{"delta":{"role":"assistant","content":"Hel"}}]}),
            json!({"id":"1","choices":[{"delta":{"content":"lo"}}]}),
            json!({"id":"1","choices":[{"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":2}}),
        ];
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/chat/completions"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_string(sse_body(&chunks))
                    .append_header("content-type", "text/event-stream"),
            )
            .mount(&server)
            .await;

        let provider = OpenAiCompatProvider::new(reqwest::Client::new());
        let mut cb = Recorder::default();
        let outcome = provider
            .stream_chat(
                Uuid::new_v4(),
                "test-model",
                &[LlmMessage::User {
                    content: "hi".into(),
                    images: vec![],
                }],
                &[],
                None,
                None,
                None,
                &ProviderCredentials {
                    base_url: server.uri(),
                    api_key: Some("k".into()),
                },
                &mut cb,
            )
            .await
            .unwrap();

        assert_eq!(outcome.text, "Hello");
        assert_eq!(cb.text.lock().unwrap().as_str(), "Hello");
        assert_eq!(outcome.finish_reason.as_deref(), Some("stop"));
        assert_eq!(outcome.usage.prompt_tokens, Some(10));
        assert_eq!(outcome.usage.completion_tokens, Some(2));
    }

    #[tokio::test]
    async fn assembles_streamed_tool_calls() {
        let chunks = vec![
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"fs_read","arguments":""}}]}}]}),
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"pa"}}]}}]}),
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"th\":\"/tmp/x\"}"}}]}}]}),
            json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}),
        ];
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_string(sse_body(&chunks))
                    .append_header("content-type", "text/event-stream"),
            )
            .mount(&server)
            .await;

        let provider = OpenAiCompatProvider::new(reqwest::Client::new());
        let mut cb = Recorder::default();
        let outcome = provider
            .stream_chat(
                Uuid::new_v4(),
                "m",
                &[LlmMessage::User {
                    content: "read".into(),
                    images: vec![],
                }],
                &[ToolDef {
                    name: "fs_read".into(),
                    description: "d".into(),
                    parameters_schema: json!({}),
                }],
                None,
                None,
                None,
                &ProviderCredentials {
                    base_url: server.uri(),
                    api_key: None,
                },
                &mut cb,
            )
            .await
            .unwrap();

        assert_eq!(outcome.tool_calls.len(), 1);
        assert_eq!(outcome.tool_calls[0].id, "call_1");
        assert_eq!(outcome.tool_calls[0].name, "fs_read");
        assert_eq!(outcome.tool_calls[0].arguments, json!({"path": "/tmp/x"}));
        assert_eq!(cb.tools.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn surfaces_upstream_error() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .respond_with(
                wiremock::ResponseTemplate::new(401)
                    .set_body_string(r#"{"error":{"message":"bad key"}}"#),
            )
            .mount(&server)
            .await;

        let provider = OpenAiCompatProvider::new(reqwest::Client::new());
        let mut cb = Recorder::default();
        let err = provider
            .stream_chat(
                Uuid::new_v4(),
                "m",
                &[],
                &[],
                None,
                None,
                None,
                &ProviderCredentials {
                    base_url: server.uri(),
                    api_key: Some("k".into()),
                },
                &mut cb,
            )
            .await
            .unwrap_err();

        match err {
            StreamError::Upstream { status, body } => {
                assert_eq!(status, 401);
                assert!(body.contains("bad key"));
            }
            other => panic!("expected upstream error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn sends_reasoning_effort_only_when_set() {
        let server = wiremock::MockServer::start().await;
        // Scoped mock: expectations are verified when the guard drops.
        let _guard = wiremock::Mock::given(wiremock::matchers::method("POST"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_string(sse_body(&[json!(
                        {"choices":[{"delta":{},"finish_reason":"stop"}]}
                    )]))
                    .append_header("content-type", "text/event-stream"),
            )
            .expect(2)
            .mount_as_scoped(&server)
            .await;

        let provider = OpenAiCompatProvider::new(reqwest::Client::new());
        let creds = ProviderCredentials {
            base_url: server.uri(),
            api_key: Some("k".into()),
        };

        // With effort set → body carries `reasoning_effort`.
        let mut cb = Recorder::default();
        provider
            .stream_chat(
                Uuid::new_v4(),
                "m",
                &[],
                &[],
                None,
                None,
                Some("high"),
                &creds,
                &mut cb,
            )
            .await
            .unwrap();

        let requests = server.received_requests().await.unwrap();
        let body: Value = serde_json::from_slice(&requests.last().unwrap().body).unwrap();
        assert_eq!(body.get("reasoning_effort"), Some(&json!("high")));

        // Without effort → field absent.
        let mut cb = Recorder::default();
        provider
            .stream_chat(
                Uuid::new_v4(),
                "m",
                &[],
                &[],
                None,
                None,
                None,
                &creds,
                &mut cb,
            )
            .await
            .unwrap();
        let requests = server.received_requests().await.unwrap();
        let body: Value = serde_json::from_slice(&requests.last().unwrap().body).unwrap();
        assert!(body.get("reasoning_effort").is_none());
    }
}
