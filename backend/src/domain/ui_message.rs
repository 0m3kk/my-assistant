use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Wire-compatible projection of the AI SDK v5 `UIMessage`.
///
/// `parts` is kept as raw JSON on purpose: the SDK evolves part shapes and the
/// server only needs to (a) store them verbatim, (b) extract text for titles /
/// LLM assembly, (c) interpret tool parts for the approval state machine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UIMessage {
    pub id: String,
    pub role: String, // "user" | "assistant"
    #[serde(default)]
    pub parts: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

/// A tool part as understood by the server (`type` is `tool-<toolName>`).
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ToolPartView {
    /// Index of the part within `message.parts`.
    pub part_index: usize,
    pub tool_call_id: String,
    pub tool_name: String,
    pub state: String,
    pub input: Value,
    pub output: Option<Value>,
    pub error_text: Option<String>,
    /// `approval.approved` when the part carries an approval response.
    pub approved: Option<bool>,
}

/// A `file` part as understood by the server (AI SDK v5 file part whose
/// `url` points at this backend's download route).
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FilePartView {
    /// Index of the part within `message.parts`.
    pub part_index: usize,
    pub media_type: String,
    pub filename: Option<String>,
    /// File row id parsed from the url (`/api/files/{id}/content`).
    pub file_id: Uuid,
}

impl UIMessage {
    pub fn is_user(&self) -> bool {
        self.role == "user"
    }

    /// Concatenated text of all `text` parts.
    pub fn text(&self) -> String {
        self.parts
            .iter()
            .filter(|p| p.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|p| p.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn tool_parts(&self) -> Vec<ToolPartView> {
        self.parts
            .iter()
            .enumerate()
            .filter_map(|(i, p)| tool_part_view(i, p))
            .collect()
    }

    pub fn file_parts(&self) -> Vec<FilePartView> {
        self.parts
            .iter()
            .enumerate()
            .filter_map(|(i, p)| file_part_view(i, p))
            .collect()
    }
}

fn file_part_view(index: usize, part: &Value) -> Option<FilePartView> {
    if part.get("type").and_then(Value::as_str) != Some("file") {
        return None;
    }
    let url = part.get("url").and_then(Value::as_str)?;
    let rest = url.strip_prefix("/api/files/")?;
    let id_str = rest.strip_suffix("/content")?;
    let file_id = Uuid::parse_str(id_str).ok()?;
    Some(FilePartView {
        part_index: index,
        media_type: part
            .get("mediaType")
            .and_then(Value::as_str)
            .unwrap_or("application/octet-stream")
            .to_string(),
        filename: part
            .get("filename")
            .and_then(Value::as_str)
            .map(str::to_string),
        file_id,
    })
}

fn tool_part_view(index: usize, part: &Value) -> Option<ToolPartView> {
    let type_ = part.get("type")?.as_str()?;
    let tool_name = type_.strip_prefix("tool-")?;
    if tool_name.is_empty() {
        return None;
    }
    let tool_call_id = part.get("toolCallId")?.as_str()?.to_string();
    Some(ToolPartView {
        part_index: index,
        tool_call_id,
        tool_name: tool_name.to_string(),
        state: part
            .get("state")
            .and_then(Value::as_str)
            .unwrap_or("input-available")
            .to_string(),
        input: part.get("input").cloned().unwrap_or(Value::Null),
        output: part.get("output").cloned(),
        error_text: part
            .get("errorText")
            .and_then(Value::as_str)
            .map(str::to_string),
        approved: part
            .get("approval")
            .and_then(|a| a.get("approved"))
            .and_then(Value::as_bool),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_text() {
        let m = UIMessage {
            id: "1".into(),
            role: "user".into(),
            parts: vec![
                json!({"type":"text","text":"hello"}),
                json!({"type":"text","text":"world"}),
            ],
            metadata: None,
        };
        assert_eq!(m.text(), "hello\nworld");
    }

    #[test]
    fn extracts_file_parts() {
        let id = uuid::Uuid::nil();
        let m = UIMessage {
            id: "3".into(),
            role: "user".into(),
            parts: vec![
                json!({"type":"text","text":"look at this"}),
                json!({"type":"file","mediaType":"image/png","filename":"shot.png",
                       "url":format!("/api/files/{id}/content")}),
                json!({"type":"file","mediaType":"text/plain","url":"https://example.com/other"}),
            ],
            metadata: None,
        };
        let fps = m.file_parts();
        assert_eq!(fps.len(), 1);
        assert_eq!(fps[0].file_id, id);
        assert_eq!(fps[0].media_type, "image/png");
        assert_eq!(fps[0].filename.as_deref(), Some("shot.png"));
    }

    #[test]
    fn extracts_tool_parts() {
        let m = UIMessage {
            id: "2".into(),
            role: "assistant".into(),
            parts: vec![
                json!({"type":"text","text":"running"}),
                json!({"type":"tool-fs_write","toolCallId":"call_1","state":"approval-responded",
                       "input":{"path":"/tmp/x"},"approval":{"approved":true}}),
            ],
            metadata: None,
        };
        let tps = m.tool_parts();
        assert_eq!(tps.len(), 1);
        assert_eq!(tps[0].tool_name, "fs_write");
        assert_eq!(tps[0].approved, Some(true));
        assert_eq!(tps[0].input, json!({"path":"/tmp/x"}));
    }
}
