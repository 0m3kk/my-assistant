//! Builtin tools compiled into the backend: current_time and web_search.
//!
//! The generic `http_request`, `echo`, and absolute-path `fs_*` tools are
//! intentionally gone. HTTP work uses self-contained `http` tools; file work
//! uses `workspace.*` or `file` tools.

use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use reqwest::Url;
use serde_json::{Value, json};

use super::{ResourceRef, Tool, ToolDefinition, ToolExecContext};
use crate::error::{AppError, AppResult};

// ---------- current_time ----------

pub struct CurrentTime;

#[async_trait]
impl Tool for CurrentTime {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "current_time".into(),
            description: "Returns the current server time in UTC (RFC 3339).".into(),
            parameters_schema: json!({"type": "object", "properties": {}}),
        }
    }

    async fn execute(&self, _ctx: &ToolExecContext, _input: Value) -> AppResult<Value> {
        Ok(json!({"utc": Utc::now().to_rfc3339()}))
    }
}

// ---------- web_search (SearXNG) ----------

const DEFAULT_MAX_RESULTS: usize = 5;
const MAX_RESULTS_LIMIT: usize = 20;
const MAX_QUERY_CHARS: usize = 400;
const MAX_SNIPPET_CHARS: usize = 500;
const MAX_SUGGESTIONS: usize = 5;
const DEFAULT_TIMEOUT_MS: u64 = 15_000;

/// Builtin web search backed by a SearXNG instance (`format=json`).
///
/// Config (merged `tools.config` + `agent_tools.config`):
/// ```json
/// {
///   "base_url": "http://localhost:8080",
///   "max_results": 5,
///   "language": "en",
///   "categories": "general,news",
///   "safesearch": 1,
///   "time_range": "week",
///   "timeout_ms": 15000
/// }
/// ```
/// `base_url` falls back to the `SEARXNG_URL` environment variable.
pub struct WebSearch {
    config: Value,
    env_base_url: Option<String>,
}

impl WebSearch {
    pub fn new(config: Value, env_base_url: Option<String>) -> Self {
        Self {
            config,
            env_base_url,
        }
    }
}

/// Resolved SearXNG call options.
#[derive(Debug, Clone, PartialEq)]
pub struct SearxOptions {
    pub base_url: String,
    pub max_results: usize,
    pub language: Option<String>,
    pub categories: Option<String>,
    pub safesearch: Option<u8>,
    pub time_range: Option<String>,
    pub timeout_ms: u64,
}

fn resolve_base_url(config: &Value, env_base_url: Option<&str>) -> AppResult<String> {
    let from_config = config
        .get("base_url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let raw = from_config
        .or_else(|| env_base_url.map(str::trim).filter(|s| !s.is_empty()))
        .ok_or_else(|| {
            AppError::bad_request(
                "web_search is not configured: set SEARXNG_URL or set base_url in Admin → Tools → web_search",
            )
        })?;
    let url = Url::parse(raw)
        .map_err(|_| AppError::bad_request("web_search base_url is not a valid URL"))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(AppError::bad_request(
            "web_search base_url must be http(s) with a host",
        ));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(AppError::bad_request(
            "web_search base_url must not contain a query or fragment",
        ));
    }
    Ok(raw.trim_end_matches('/').to_string())
}

fn opt_str(config: &Value, key: &str) -> Option<String> {
    config
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn options_from(config: &Value, env_base_url: Option<&str>) -> AppResult<SearxOptions> {
    let base_url = resolve_base_url(config, env_base_url)?;
    let max_results = config
        .get("max_results")
        .and_then(Value::as_u64)
        .map(|n| (n as usize).clamp(1, MAX_RESULTS_LIMIT))
        .unwrap_or(DEFAULT_MAX_RESULTS);
    let safesearch = config
        .get("safesearch")
        .and_then(Value::as_u64)
        .map(|n| n.min(2) as u8);
    let time_range = opt_str(config, "time_range");
    if let Some(range) = &time_range {
        if !matches!(range.as_str(), "day" | "week" | "month" | "year") {
            return Err(AppError::validation(
                "web_search time_range must be day, week, month or year",
            ));
        }
    }
    let timeout_ms = config
        .get("timeout_ms")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_TIMEOUT_MS)
        .clamp(1_000, 120_000);
    Ok(SearxOptions {
        base_url,
        max_results,
        language: opt_str(config, "language"),
        categories: opt_str(config, "categories"),
        safesearch,
        time_range,
        timeout_ms,
    })
}

fn max_results_for(input: &Value, options: &SearxOptions) -> usize {
    input
        .get("max_results")
        .and_then(Value::as_u64)
        .map(|n| (n as usize).clamp(1, MAX_RESULTS_LIMIT))
        .unwrap_or(options.max_results)
}

fn search_url(options: &SearxOptions, query: &str) -> AppResult<Url> {
    let mut url = Url::parse(&options.base_url)
        .map_err(|_| AppError::bad_request("web_search base_url is not a valid URL"))?;
    let origin = url.origin().ascii_serialization();
    let path = format!("{}/search", url.path().trim_end_matches('/'));
    url.set_path(&path);
    url.set_query(None);
    url.set_fragment(None);
    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("q", query);
        pairs.append_pair("format", "json");
        pairs.append_pair("pageno", "1");
        if let Some(language) = &options.language {
            pairs.append_pair("language", language);
        }
        if let Some(categories) = &options.categories {
            pairs.append_pair("categories", categories);
        }
        if let Some(safesearch) = options.safesearch {
            pairs.append_pair("safesearch", &safesearch.to_string());
        }
        if let Some(time_range) = &options.time_range {
            pairs.append_pair("time_range", time_range);
        }
    }
    if url.origin().ascii_serialization() != origin {
        return Err(AppError::forbidden(
            "searxng URL escaped the configured host",
        ));
    }
    Ok(url)
}

async fn search_searx(
    client: &reqwest::Client,
    options: &SearxOptions,
    query: &str,
    max_results: usize,
) -> AppResult<Value> {
    let url = search_url(options, query)?;
    let response = client
        .get(url)
        .header("accept", "application/json")
        .timeout(Duration::from_millis(options.timeout_ms))
        .send()
        .await
        .map_err(|e| AppError::bad_request(format!("searxng request failed: {e}")))?;
    let status = response.status();
    if !status.is_success() {
        return Err(AppError::bad_request(format!(
            "searxng returned HTTP {}",
            status.as_u16()
        )));
    }
    let body: Value = response.json().await.map_err(|_| {
        AppError::bad_request(
            "searxng returned a non-JSON response — enable `json` in search.formats",
        )
    })?;
    Ok(normalize_searx(&body, max_results, query))
}

fn normalize_searx(body: &Value, max_results: usize, query: &str) -> Value {
    let mut results = Vec::new();
    for item in body
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if results.len() >= max_results {
            break;
        }
        let Some(url) = item
            .get("url")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        let title = item
            .get("title")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(url);
        let mut entry = json!({"title": title, "url": url});
        if let Some(snippet) = item
            .get("content")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            entry["snippet"] = json!(truncate_chars(snippet, MAX_SNIPPET_CHARS));
        }
        if let Some(published) = item
            .get("publishedDate")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            entry["published_at"] = json!(published);
        }
        results.push(entry);
    }

    let mut output = json!({"provider": "searxng", "query": query, "results": results});
    let answers = string_list(body.get("answers"));
    if !answers.is_empty() {
        output["answers"] = json!(answers);
    }
    let mut suggestions = string_list(body.get("suggestions"));
    suggestions.truncate(MAX_SUGGESTIONS);
    if !suggestions.is_empty() {
        output["suggestions"] = json!(suggestions);
    }
    output
}

fn string_list(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn truncate_chars(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        value.to_string()
    } else {
        value.chars().take(max).collect()
    }
}

#[async_trait]
impl Tool for WebSearch {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "web_search".into(),
            description: "Searches the web through the configured SearXNG instance. Returns \
                          ranked results with title, url and snippet. Use it for current facts and \
                          cite the source URLs in your answer."
                .into(),
            parameters_schema: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Search query"},
                    "max_results": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 20,
                        "description": "How many results to return (default 5)"
                    }
                },
                "required": ["query"],
            }),
        }
    }

    fn resources(&self, _input: &Value) -> Vec<ResourceRef> {
        options_from(&self.config, self.env_base_url.as_deref())
            .ok()
            .and_then(|options| Url::parse(&options.base_url).ok())
            .and_then(|url| url.host_str().map(|host| vec![ResourceRef::domain(host)]))
            .unwrap_or_default()
    }

    async fn execute(&self, ctx: &ToolExecContext, input: Value) -> AppResult<Value> {
        let query = input
            .get("query")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| AppError::bad_request("query is required"))?;
        let query = truncate_chars(query, MAX_QUERY_CHARS);
        let options = options_from(&self.config, self.env_base_url.as_deref())?;
        let max_results = max_results_for(&input, &options);
        if ctx.cancel.is_cancelled() {
            return Err(AppError::bad_request("run cancelled"));
        }
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_millis(options.timeout_ms))
            .build()
            .map_err(|e| AppError::internal(format!("http client error: {e}")))?;
        search_searx(&client, &options, &query, max_results).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn sample_body() -> Value {
        json!({
            "query": "rust",
            "results": [
                {"url": "https://a.example/1", "title": "A", "content": "alpha", "publishedDate": "2026-01-02T00:00:00"},
                {"url": "https://b.example/2", "title": "", "content": "beta"},
                {"title": "no url", "content": "skip"},
                {"url": "https://c.example/3", "title": "C", "content": "x".repeat(900)}
            ],
            "answers": ["42"],
            "suggestions": ["s1", "s2", "s3", "s4", "s5", "s6"]
        })
    }

    #[test]
    fn normalize_truncates_and_skips_invalid_results() {
        let out = normalize_searx(&sample_body(), 2, "rust");
        assert_eq!(out["provider"], "searxng");
        assert_eq!(out["query"], "rust");
        let results = out["results"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["title"], "A");
        assert_eq!(results[0]["published_at"], "2026-01-02T00:00:00");
        assert_eq!(results[1]["title"], "https://b.example/2");
        assert_eq!(out["answers"][0], "42");
        assert_eq!(
            out["suggestions"].as_array().unwrap().len(),
            MAX_SUGGESTIONS
        );
    }

    #[test]
    fn normalize_caps_snippets_and_handles_empty_body() {
        let out = normalize_searx(&sample_body(), 10, "rust");
        let results = out["results"].as_array().unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(
            results[2]["snippet"].as_str().unwrap().chars().count(),
            MAX_SNIPPET_CHARS
        );
        let empty = normalize_searx(&json!({"results": []}), 5, "q");
        assert!(empty["results"].as_array().unwrap().is_empty());
        assert!(empty.get("answers").is_none());
        assert!(empty.get("suggestions").is_none());
    }

    #[test]
    fn options_default_and_precedence() {
        let from_env = options_from(&json!({}), Some("http://localhost:8080/")).unwrap();
        assert_eq!(from_env.base_url, "http://localhost:8080");
        assert_eq!(from_env.max_results, DEFAULT_MAX_RESULTS);
        assert_eq!(from_env.timeout_ms, DEFAULT_TIMEOUT_MS);

        let from_config = options_from(
            &json!({
                "base_url": "http://searx.internal:8081",
                "max_results": 50,
                "language": "vi",
                "safesearch": 9,
                "time_range": "week"
            }),
            Some("http://ignored:8080"),
        )
        .unwrap();
        assert_eq!(from_config.base_url, "http://searx.internal:8081");
        assert_eq!(from_config.max_results, MAX_RESULTS_LIMIT);
        assert_eq!(from_config.safesearch, Some(2));
        assert_eq!(from_config.language.as_deref(), Some("vi"));
        assert_eq!(from_config.time_range.as_deref(), Some("week"));
    }

    #[test]
    fn options_require_a_valid_base_url() {
        assert!(options_from(&json!({}), None).is_err());
        let err = options_from(&json!({"base_url": "ftp://example.com"}), None).unwrap_err();
        assert!(err.to_string().contains("http(s)"));
        assert!(options_from(&json!({"time_range": "decade"}), Some("http://x:8080")).is_err());
    }

    #[test]
    fn max_results_input_override_is_clamped() {
        let options = options_from(&json!({"max_results": 3}), Some("http://x:8080")).unwrap();
        assert_eq!(max_results_for(&json!({}), &options), 3);
        assert_eq!(max_results_for(&json!({"max_results": 0}), &options), 1);
        assert_eq!(
            max_results_for(&json!({"max_results": 99}), &options),
            MAX_RESULTS_LIMIT
        );
    }

    #[tokio::test]
    async fn calls_searxng_json_endpoint() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/search"))
            .and(query_param("q", "axum sse"))
            .and(query_param("format", "json"))
            .and(query_param("language", "en"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [{"url": "https://example.com", "title": "Example", "content": "hello"}]
            })))
            .mount(&server)
            .await;

        let options =
            options_from(&json!({"base_url": server.uri(), "language": "en"}), None).unwrap();
        let client = reqwest::Client::new();
        let out = search_searx(&client, &options, "axum sse", 5)
            .await
            .unwrap();
        assert_eq!(out["results"][0]["url"], "https://example.com");
    }

    #[tokio::test]
    async fn reports_http_and_non_json_failures() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/search"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        let options = options_from(&json!({"base_url": server.uri()}), None).unwrap();
        let client = reqwest::Client::new();
        let err = search_searx(&client, &options, "q", 5).await.unwrap_err();
        assert!(err.to_string().contains("503"));

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/search"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>nope</html>"))
            .mount(&server)
            .await;
        let options = options_from(&json!({"base_url": server.uri()}), None).unwrap();
        let err = search_searx(&client, &options, "q", 5).await.unwrap_err();
        assert!(err.to_string().contains("search.formats"));
    }
}
