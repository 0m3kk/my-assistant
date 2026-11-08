pub mod ui_message;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------- Users & preferences ----------

#[derive(Debug, Clone, sqlx::FromRow)]
#[allow(dead_code)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    #[allow(dead_code)]
    pub password_hash: String,
    pub name: String,
    pub avatar_url: Option<String>,
    pub role: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct PublicUser {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    pub avatar_url: Option<String>,
    pub role: String,
}

impl From<&User> for PublicUser {
    fn from(u: &User) -> Self {
        Self {
            id: u.id,
            email: u.email.clone(),
            name: u.name.clone(),
            avatar_url: u.avatar_url.clone(),
            role: u.role.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct UserPreferences {
    pub user_id: Uuid,
    pub theme: String,
    pub font_scale: String,
    pub locale: String,
    /// Model used to generate conversation titles (None = global default).
    pub title_model_id: Option<Uuid>,
    pub updated_at: DateTime<Utc>,
}

// ---------- Secrets ----------

#[derive(Debug, Clone, sqlx::FromRow)]
#[allow(dead_code)]
pub struct SecretRow {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    #[allow(dead_code)]
    pub value_encrypted: Vec<u8>,
    #[allow(dead_code)]
    pub nonce: Vec<u8>,
    pub version: i32,
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Admin-facing projection — never carries the value.
#[derive(Debug, Serialize)]
pub struct SecretPublic {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub masked: String,
    pub version: i32,
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ---------- Providers & models ----------

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct Provider {
    pub id: Uuid,
    pub name: String,
    pub preset: String,
    pub base_url: String,
    pub secret_id: Option<Uuid>,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct Model {
    pub id: Uuid,
    pub provider_id: Uuid,
    pub api_name: String,
    pub display_name: String,
    pub context_window: Option<i32>,
    pub capabilities: serde_json::Value,
    pub reasoning_effort: Option<String>,
    pub is_default: bool,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Model {
    #[allow(dead_code)]
    pub fn supports_tools(&self) -> bool {
        self.capabilities
            .get("tools")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    /// Whether the model accepts image inputs (multimodal vision).
    pub fn supports_vision(&self) -> bool {
        self.capabilities
            .get("vision")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }
}

/// Valid reasoning effort values on the OpenAI-compatible wire format.
const REASONING_EFFORTS: &[&str] = &["low", "medium", "high", "xhigh"];

/// Normalize a client-supplied reasoning effort: empty → None (unset),
/// unknown value → Err message. Case-insensitive.
pub fn normalize_effort(raw: &str) -> Result<Option<String>, String> {
    let t = raw.trim().to_lowercase();
    if t.is_empty() {
        return Ok(None);
    }
    if REASONING_EFFORTS.contains(&t.as_str()) {
        Ok(Some(t))
    } else {
        Err(format!(
            "reasoning_effort must be one of: {}",
            REASONING_EFFORTS.join(", ")
        ))
    }
}

/// Static provider presets served to the admin UI for form prefill.
#[derive(Debug, Serialize)]
pub struct ProviderPreset {
    pub id: &'static str,
    pub label: &'static str,
    pub base_url: &'static str,
    pub hint: &'static str,
}

pub const PROVIDER_PRESETS: &[ProviderPreset] = &[
    ProviderPreset {
        id: "glm",
        label: "GLM (Z.ai)",
        base_url: "https://api.z.ai/api/paas/v4",
        hint: "Z.ai OpenAI-compatible endpoint. Models: glm-4.6, glm-4.5-air, …",
    },
    ProviderPreset {
        id: "opencode",
        label: "OpenCode",
        base_url: "https://opencode.ai/zen/v1",
        hint: "OpenCode gateway (OpenAI-compatible).",
    },
    ProviderPreset {
        id: "openai",
        label: "OpenAI",
        base_url: "https://api.openai.com/v1",
        hint: "Official OpenAI API.",
    },
    ProviderPreset {
        id: "openrouter",
        label: "OpenRouter",
        base_url: "https://openrouter.ai/api/v1",
        hint: "Multi-provider router. Models: openai/gpt-4o, anthropic/claude-…",
    },
    ProviderPreset {
        id: "ollama",
        label: "Ollama (local)",
        base_url: "http://localhost:11434/v1",
        hint: "Local Ollama server, no API key required.",
    },
    ProviderPreset {
        id: "groq",
        label: "Groq",
        base_url: "https://api.groq.com/openai/v1",
        hint: "Groq fast inference.",
    },
    ProviderPreset {
        id: "custom",
        label: "Custom endpoint",
        base_url: "",
        hint: "Any OpenAI Chat Completions compatible endpoint. Set base URL manually.",
    },
];

// ---------- Agents & tools ----------

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct Agent {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub description: String,
    pub system_prompt: String,
    pub default_model_id: Option<Uuid>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<i32>,
    pub max_tool_calls: i32,
    pub kind: String,
    pub is_builtin: bool,
    pub revision: i32,
    pub status: String,
    pub group_id: Option<Uuid>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct AgentGroup {
    pub id: Uuid,
    pub name: String,
    pub position: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct ToolRow {
    pub id: Uuid,
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub kind: String,
    pub source_ref: String,
    pub config: serde_json::Value,
    pub parameters_schema: serde_json::Value,
    pub requires_approval: bool,
    pub is_meta: bool,
    pub is_builtin: bool,
    pub revision: i32,
    pub status: String,
    pub group_id: Option<Uuid>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct ToolGroup {
    pub id: Uuid,
    pub name: String,
    pub position: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
#[allow(dead_code)]
pub struct AgentToolRow {
    pub agent_id: Uuid,
    pub tool_id: Uuid,
    pub requires_approval: Option<bool>,
    pub config: Option<serde_json::Value>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct ToolBuild {
    pub id: Uuid,
    pub tool_id: Uuid,
    pub revision: i32,
    pub language: String,
    pub source: String,
    pub runtime: String,
    pub status: String,
    pub storage_key: Option<String>,
    pub sha256: Option<String>,
    pub size_bytes: Option<i64>,
    pub build_log: Option<String>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

// ---------- Conversations, runs & messages ----------

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct Conversation {
    pub id: Uuid,
    pub user_id: Uuid,
    pub agent_id: Uuid,
    pub title: String,
    pub model_id: Option<Uuid>,
    /// False while the title is the first-message heuristic; true once it has
    /// been generated (or set manually) and must no longer be overwritten.
    pub title_generated: bool,
    pub status: String,
    pub group_id: Option<Uuid>,
    pub last_message_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct ConversationGroup {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub position: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct Run {
    pub id: Uuid,
    pub conversation_id: Uuid,
    pub agent_id: Uuid,
    pub model_id: Option<Uuid>,
    pub assistant_message_id: Option<Uuid>,
    pub status: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
#[allow(dead_code)]
pub struct MessageRow {
    pub id: Uuid,
    pub conversation_id: Uuid,
    pub run_id: Option<Uuid>,
    pub role: String,
    pub parts: serde_json::Value,
    pub model_id: Option<Uuid>,
    pub usage: Option<serde_json::Value>,
    pub seq: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct ToolExecution {
    pub id: Uuid,
    pub run_id: Option<Uuid>,
    pub conversation_id: Uuid,
    pub tool_call_id: String,
    pub tool_id: Option<Uuid>,
    pub tool_name: String,
    pub input: serde_json::Value,
    pub output: Option<serde_json::Value>,
    pub status: String,
    pub error_text: Option<String>,
    pub duration_ms: Option<i32>,
    pub resources_used: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

// ---------- Approvals ----------

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct Approval {
    pub id: Uuid,
    pub conversation_id: Uuid,
    pub tool_call_id: String,
    pub tool_name: String,
    pub input: serde_json::Value,
    pub reason: String,
    pub requested_resource: Option<String>,
    pub status: String,
    pub decision_mode: Option<String>,
    pub decided_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

// ---------- Change proposals ----------

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct ChangeProposal {
    pub id: Uuid,
    pub resource: String,
    pub action: String,
    pub target_id: Option<Uuid>,
    pub status: String,
    pub title: String,
    pub rationale: String,
    pub payload: serde_json::Value,
    pub before: Option<serde_json::Value>,
    pub feedback_ids: Vec<Uuid>,
    pub created_by_user_id: Option<Uuid>,
    pub created_by_agent_id: Option<Uuid>,
    pub conversation_id: Option<Uuid>,
    pub reviewed_by: Option<Uuid>,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub applied_at: Option<DateTime<Utc>>,
    pub apply_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ---------- Feedback ----------

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct FeedbackRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub scope: String,
    pub conversation_id: Option<Uuid>,
    pub run_id: Option<Uuid>,
    pub tool_execution_id: Option<Uuid>,
    pub tool_id: Option<Uuid>,
    pub agent_id: Option<Uuid>,
    pub rating: i16,
    pub comment: String,
    pub status: String,
    pub addressed_by_proposal_id: Option<Uuid>,
    pub subject_key: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ---------- Files (attachments + artifacts) ----------

/// Database row in the `files` table (attachments + artifacts).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct FileRow {
    pub id: Uuid,
    pub conversation_id: Option<Uuid>,
    pub message_id: Option<Uuid>,
    pub user_id: Uuid,
    pub origin: String,
    pub filename: String,
    pub content_type: String,
    pub size_bytes: i64,
    #[allow(dead_code)]
    pub storage_key: String,
    pub checksum_sha256: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl FileRow {
    /// Client-facing projection. `url` points at the authed download route.
    pub fn to_dto(&self) -> FileDto {
        FileDto {
            id: self.id,
            origin: self.origin.clone(),
            filename: self.filename.clone(),
            content_type: self.content_type.clone(),
            size_bytes: self.size_bytes,
            checksum_sha256: self.checksum_sha256.clone(),
            url: format!("/api/files/{}/content", self.id),
            created_at: self.created_at,
        }
    }
}

/// Client-facing file metadata (API DTO, snake_case like the rest).
#[derive(Debug, Clone, Serialize)]
pub struct FileDto {
    pub id: Uuid,
    pub origin: String,
    pub filename: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub checksum_sha256: Option<String>,
    pub url: String,
    pub created_at: DateTime<Utc>,
}

/// One entry of the conversation workspace (sandbox) tree.
#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceEntry {
    /// Path relative to the conversation sandbox root, `/`-separated.
    pub path: String,
    pub is_dir: bool,
    pub size_bytes: Option<i64>,
    pub modified_at: Option<DateTime<Utc>>,
}

// ---------- Usage & audit ----------

#[derive(Debug, Clone, sqlx::FromRow)]
#[allow(dead_code)]
pub struct UsageLog {
    pub id: Uuid,
    pub user_id: Uuid,
    pub conversation_id: Option<Uuid>,
    pub model_id: Option<Uuid>,
    pub prompt_tokens: Option<i32>,
    pub completion_tokens: Option<i32>,
    pub created_at: DateTime<Utc>,
}
