//! `http` tools: a self-contained endpoint connector.
//!
//! Config (stored in `tools.config`):
//! ```json
//! {
//!   "base_url": "https://api.github.com",
//!   "method": "GET",
//!   "path_template": "/repos/{owner}/{repo}/pulls/{number}",
//!   "query": {"per_page": "100"},
//!   "headers": {"Authorization": "Bearer {{secret:github_token}}"},
//!   "body_template": null,
//!   "response_map": "data.items",
//!   "timeout_ms": 20000,
//!   "max_output_bytes": 262144
//! }
//! ```
//! `{param}` placeholders come from the schema input. `{{secret:NAME}}`
//! references are resolved at execute time and never reach the model.

use std::time::Duration;

use async_trait::async_trait;
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use serde_json::{Value, json};

use super::{ResourceRef, Tool, ToolDefinition, ToolExecContext};
use crate::error::{AppError, AppResult};

const PATH_ENCODE: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}')
    .add(b'/')
    .add(b'%');

const QUERY_ENCODE: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'<')
    .add(b'>')
    .add(b'&')
    .add(b'=')
    .add(b'+')
    .add(b'%');

pub struct HttpTool {
    pub name: String,
    pub description: String,
    pub parameters_schema: Value,
    pub config: Value,
}

impl HttpTool {
    fn base_url(&self) -> AppResult<reqwest::Url> {
        let raw = self
            .config
            .get("base_url")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::internal("http tool config missing base_url"))?;
        reqwest::Url::parse(raw).map_err(|_| AppError::internal("http tool base_url is invalid"))
    }

    fn method(&self) -> String {
        self.config
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("GET")
            .to_uppercase()
    }

    fn timeout(&self) -> Duration {
        Duration::from_millis(
            self.config
                .get("timeout_ms")
                .and_then(Value::as_u64)
                .unwrap_or(20_000),
        )
    }
}

fn render_template(template: &str, input: &Value, encode: &'static AsciiSet) -> AppResult<String> {
    let mut out = String::with_capacity(template.len());
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            if let Some(rel_end) = template[i + 1..].find('}') {
                let end = i + 1 + rel_end;
                let key = &template[i + 1..end];
                if !key.is_empty() && !key.contains('{') {
                    let value = input.get(key).ok_or_else(|| {
                        AppError::bad_request(format!("missing parameter: {key}"))
                    })?;
                    let raw = match value {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    out.push_str(&utf8_percent_encode(&raw, encode).to_string());
                    i = end + 1;
                    continue;
                }
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    Ok(out)
}

fn substitute_raw(template: &str, input: &Value) -> AppResult<String> {
    let mut out = String::with_capacity(template.len());
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            if let Some(rel_end) = template[i + 1..].find('}') {
                let end = i + 1 + rel_end;
                let key = &template[i + 1..end];
                if !key.is_empty() && !key.contains('{') {
                    let value = input.get(key).ok_or_else(|| {
                        AppError::bad_request(format!("missing parameter: {key}"))
                    })?;
                    let raw = match value {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    out.push_str(&raw);
                    i = end + 1;
                    continue;
                }
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    Ok(out)
}

fn substitute_json(template: &str, input: &Value) -> AppResult<String> {
    let mut out = String::with_capacity(template.len());
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            if let Some(rel_end) = template[i + 1..].find('}') {
                let end = i + 1 + rel_end;
                let key = &template[i + 1..end];
                if !key.is_empty() && !key.contains('{') {
                    let value = input.get(key).ok_or_else(|| {
                        AppError::bad_request(format!("missing parameter: {key}"))
                    })?;
                    let raw = match value {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    let quoted = json!(raw).to_string();
                    out.push_str(&quoted[1..quoted.len() - 1]);
                    i = end + 1;
                    continue;
                }
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    Ok(out)
}

#[async_trait]
impl Tool for HttpTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name.clone(),
            description: self.description.clone(),
            parameters_schema: self.parameters_schema.clone(),
        }
    }

    fn resources(&self, _input: &Value) -> Vec<ResourceRef> {
        self.base_url()
            .ok()
            .and_then(|u| u.host_str().map(|h| vec![ResourceRef::domain(h)]))
            .unwrap_or_default()
    }

    async fn execute(&self, ctx: &ToolExecContext, input: Value) -> AppResult<Value> {
        let base = self.base_url()?;
        let base_origin = base.origin().ascii_serialization();

        let path_template = self
            .config
            .get("path_template")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::internal("http tool config missing path_template"))?;
        let path = render_template(path_template, &input, PATH_ENCODE)?;

        let mut url = base.clone();
        url.set_path(&format!("{}{}", base.path().trim_end_matches('/'), path));
        url.set_query(None);

        if let Some(query) = self.config.get("query").and_then(Value::as_object) {
            let mut pairs: Vec<(String, String)> = Vec::new();
            for (k, v) in query {
                let raw = match v {
                    Value::String(s) => render_template(s, &input, QUERY_ENCODE)?,
                    other => render_template(&other.to_string(), &input, QUERY_ENCODE)?,
                };
                pairs.push((k.clone(), raw));
            }
            if !pairs.is_empty() {
                let mut qs = url.query_pairs_mut();
                for (k, v) in pairs {
                    qs.append_pair(&k, &v);
                }
            }
        }

        if url.origin().ascii_serialization() != base_origin {
            return Err(AppError::forbidden(
                "rendered URL escaped the configured base host",
            ));
        }

        let method = reqwest::Method::from_bytes(self.method().as_bytes())
            .map_err(|_| AppError::internal("invalid HTTP method in tool config"))?;
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(self.timeout())
            .build()
            .map_err(|e| AppError::internal(format!("http client error: {e}")))?;
        let mut req = client.request(method, url);

        if let Some(headers) = self.config.get("headers").and_then(Value::as_object) {
            for (k, v) in headers {
                let Some(v) = v.as_str() else { continue };
                let value = substitute_raw(v, &input)?;
                let value = ctx.secrets.resolve_str(&value).await?;
                req = req.header(k, value);
            }
        }

        if let Some(body_template) = self.config.get("body_template").and_then(Value::as_str) {
            let body = substitute_json(body_template, &input)?;
            let body = ctx.secrets.resolve_str(&body).await?;
            req = req.header("content-type", "application/json").body(body);
        }

        let response = req
            .send()
            .await
            .map_err(|e| AppError::bad_request(format!("request failed: {e}")))?;
        let status = response.status().as_u16();
        let text = response.text().await.unwrap_or_default();

        let parsed: Value = serde_json::from_str(&text).unwrap_or(Value::String(text));
        let body = match self.config.get("response_map").and_then(Value::as_str) {
            Some(path) => super::http_tool::json_get_path(&parsed, path)
                .cloned()
                .unwrap_or(Value::Null),
            None => parsed,
        };

        Ok(json!({"status": status, "body": body}))
    }
}

/// Minimal dot-path extraction: `a.b[0].c`. Returns None when the path misses.
pub fn json_get_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;
    for segment in path.split('.') {
        if segment.is_empty() {
            continue;
        }
        let (key, index) = match segment.split_once('[') {
            Some((k, rest)) => {
                let idx: String = rest.trim_end_matches(']').chars().collect();
                (k, idx.parse::<usize>().ok())
            }
            None => (segment, None),
        };
        if !key.is_empty() {
            current = current.get(key)?;
        }
        if let Some(i) = index {
            current = current.get(i)?;
        }
    }
    Some(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_params_are_encoded() {
        let out = render_template(
            "/repos/{owner}/{repo}/pulls/{number}",
            &json!({"owner": "a b", "repo": "x/y", "number": 12}),
            PATH_ENCODE,
        )
        .unwrap();
        assert_eq!(out, "/repos/a%20b/x%2Fy/pulls/12");
    }

    #[test]
    fn dot_path() {
        let v = json!({"data": {"items": [{"temp_c": 31.5}]}});
        assert_eq!(
            json_get_path(&v, "data.items[0].temp_c"),
            Some(&json!(31.5))
        );
        assert_eq!(json_get_path(&v, "data.missing"), None);
    }
}
