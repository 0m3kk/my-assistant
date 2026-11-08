pub mod openaicompat;

pub use anyhow_like::StreamError;

use async_trait::async_trait;
use serde_json::Value;
use uuid::Uuid;

/// A single tool call requested by the model.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCallReq {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

/// Tool definition advertised to the model.
#[derive(Debug, Clone)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub parameters_schema: Value,
}

#[derive(Debug, Clone)]
pub enum LlmMessage {
    System {
        content: String,
    },
    /// User turn. `content` is the plain text; `images` are data URLs or
    /// absolute URLs attached to the message (multimodal vision input) —
    /// providers that cannot send them simply ignore the extra blocks.
    User {
        content: String,
        images: Vec<String>,
    },
    Assistant {
        content: Option<String>,
        tool_calls: Vec<ToolCallReq>,
    },
    ToolResult {
        tool_call_id: String,
        content: String,
    },
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Usage {
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
}

/// What the provider produced during one streaming call.
#[derive(Debug, Clone, Default)]
pub struct StreamOutcome {
    pub text: String,
    pub reasoning: String,
    pub tool_calls: Vec<ToolCallReq>,
    pub usage: Usage,
    pub finish_reason: Option<String>,
}

/// Callbacks invoked while the provider streams. Implementors forward chunks
/// into `aistream` and accumulate parts for persistence.
#[allow(unused_variables)]
pub trait StreamCallbacks: Send {
    fn on_text_delta(&mut self, delta: &str) {}
    fn on_reasoning_delta(&mut self, delta: &str) {}
    /// Fired once per complete tool call (after the stream finished assembling it).
    fn on_tool_call(&mut self, call: &ToolCallReq) {}
}

/// A no-op callback set, useful in tests.
#[allow(dead_code)]
pub struct NullCallbacks;
impl StreamCallbacks for NullCallbacks {}

/// Per-call credentials — resolved from the secrets layer before invocation.
#[derive(Debug, Clone)]
pub struct ProviderCredentials {
    pub base_url: String,
    pub api_key: Option<String>,
}

/// A model advertised by the provider's model-listing endpoint (`GET /models`).
#[derive(Debug, Clone)]
pub struct DiscoveredModel {
    /// Wire name to use as `models.api_name`.
    pub api_name: String,
    /// Human-friendly label; falls back to `api_name` when the provider has none.
    pub display_name: String,
    /// Only present when the provider exposes it (e.g. OpenRouter `context_length`).
    pub context_window: Option<i32>,
}

#[async_trait]
#[allow(clippy::too_many_arguments)]
pub trait Provider: Send + Sync {
    /// Stable provider type id (`openaicompat`, …) used by the registry.
    fn id(&self) -> &'static str;

    /// List the models the provider exposes. Adapters that cannot discover
    /// models keep this default (surfaced to the admin UI as "not supported").
    async fn list_models(
        &self,
        _credentials: &ProviderCredentials,
    ) -> anyhow_like::Result<Vec<DiscoveredModel>> {
        Err(StreamError::Config(format!(
            "provider '{}' does not support model discovery",
            self.id()
        )))
    }

    async fn stream_chat(
        &self,
        run_id: Uuid,
        model: &str,
        messages: &[LlmMessage],
        tools: &[ToolDef],
        temperature: Option<f32>,
        max_tokens: Option<i32>,
        reasoning_effort: Option<&str>,
        credentials: &ProviderCredentials,
        callbacks: &mut dyn StreamCallbacks,
    ) -> anyhow_like::Result<StreamOutcome>;
}

/// Tiny local result alias so the llm module has no dependency on `AppError`
/// (providers can fail for transport reasons that map to different statuses).
pub mod anyhow_like {
    #[derive(Debug, thiserror::Error)]
    #[allow(dead_code)] // full error surface for future adapters
    pub enum StreamError {
        #[error("provider configuration error: {0}")]
        Config(String),
        #[error("upstream provider error ({status}): {body}")]
        Upstream { status: u16, body: String },
        #[error("network error: {0}")]
        Network(String),
        #[error("malformed provider response: {0}")]
        Malformed(String),
        #[error("operation cancelled")]
        Cancelled,
    }

    pub type Result<T> = std::result::Result<T, StreamError>;
}
