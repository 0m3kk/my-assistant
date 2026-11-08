//! UI Message Stream protocol (AI SDK v5) — chunk types + SSE encoding.
//!
//! Transport-agnostic by design: [`Chunk`] is a serde type; adapters turn a
//! stream of chunks into SSE events (axum) or anything else later (WebSocket).
//!
//! Chunk shapes mirror the `UIMessageChunk` union of the `ai` package. When the
//! frontend SDK is pinned, run `cargo test` golden tests against its source of
//! truth (`node_modules/ai/dist/index.d.ts`) and update here if it drifted.

use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)] // full protocol surface; not all chunks emitted yet
pub enum Chunk {
    Start,
    StartStep,
    FinishStep,
    Finish,
    TextStart {
        id: String,
    },
    TextDelta {
        id: String,
        delta: String,
    },
    TextEnd {
        id: String,
    },
    ReasoningStart {
        id: String,
    },
    ReasoningDelta {
        id: String,
        delta: String,
    },
    ReasoningEnd {
        id: String,
    },
    ToolInputStart {
        tool_call_id: String,
        tool_name: String,
    },
    ToolInputAvailable {
        tool_call_id: String,
        tool_name: String,
        input: Value,
    },
    ToolOutputAvailable {
        tool_call_id: String,
        output: Value,
    },
    ToolOutputError {
        tool_call_id: String,
        error_text: String,
    },
    /// Ask the human to approve a pending tool call.
    ToolApprovalRequest {
        approval_id: String,
        tool_call_id: String,
    },
    /// The user denied the tool call (server-side ledger decision).
    ToolOutputDenied {
        tool_call_id: String,
    },
    /// The run was cancelled (user abort) — client stops the pending turn.
    Abort,
    /// Custom data part; serialized as `data-{name}` (custom UI on the chat surface).
    Data {
        name: String,
        id: String,
        data: Value,
    },
    Error {
        error_text: String,
    },
}

impl Chunk {
    /// JSON wire form of this chunk.
    pub fn to_json(&self) -> Value {
        match self {
            Chunk::Start => json!({"type": "start"}),
            Chunk::StartStep => json!({"type": "start-step"}),
            Chunk::FinishStep => json!({"type": "finish-step"}),
            Chunk::Finish => json!({"type": "finish"}),
            Chunk::TextStart { id } => json!({"type": "text-start", "id": id}),
            Chunk::TextDelta { id, delta } => {
                json!({"type": "text-delta", "id": id, "delta": delta})
            }
            Chunk::TextEnd { id } => json!({"type": "text-end", "id": id}),
            Chunk::ReasoningStart { id } => json!({"type": "reasoning-start", "id": id}),
            Chunk::ReasoningDelta { id, delta } => {
                json!({"type": "reasoning-delta", "id": id, "delta": delta})
            }
            Chunk::ReasoningEnd { id } => json!({"type": "reasoning-end", "id": id}),
            Chunk::ToolInputStart {
                tool_call_id,
                tool_name,
            } => {
                json!({"type": "tool-input-start", "toolCallId": tool_call_id, "toolName": tool_name})
            }
            Chunk::ToolInputAvailable {
                tool_call_id,
                tool_name,
                input,
            } => {
                json!({"type": "tool-input-available", "toolCallId": tool_call_id, "toolName": tool_name, "input": input})
            }
            Chunk::ToolOutputAvailable {
                tool_call_id,
                output,
            } => {
                json!({"type": "tool-output-available", "toolCallId": tool_call_id, "output": output})
            }
            Chunk::ToolOutputError {
                tool_call_id,
                error_text,
            } => {
                json!({"type": "tool-output-error", "toolCallId": tool_call_id, "errorText": error_text})
            }
            Chunk::ToolApprovalRequest {
                approval_id,
                tool_call_id,
            } => {
                json!({"type": "tool-approval-request", "approvalId": approval_id, "toolCallId": tool_call_id})
            }
            Chunk::ToolOutputDenied { tool_call_id } => {
                json!({"type": "tool-output-denied", "toolCallId": tool_call_id})
            }
            Chunk::Abort => json!({"type": "abort"}),
            Chunk::Data { name, id, data } => {
                json!({"type": format!("data-{name}"), "id": id, "data": data})
            }
            Chunk::Error { error_text } => json!({"type": "error", "errorText": error_text}),
        }
    }

    /// Compact single-line JSON (SSE data payload must not contain raw newlines).
    pub fn to_wire(&self) -> String {
        self.to_json().to_string()
    }
}

/// Incremental text part id generator — stable short ids (`t0`, `t1`, …).
#[derive(Debug, Default)]
pub struct IdGen {
    prefix: &'static str,
    next: usize,
}

impl IdGen {
    pub fn new(prefix: &'static str) -> Self {
        Self { prefix, next: 0 }
    }

    pub fn next(&mut self) -> String {
        let id = format!("{}{}", self.prefix, self.next);
        self.next += 1;
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_chunk_shapes() {
        let cases: Vec<(Chunk, &str)> = vec![
            (Chunk::Start, r#"{"type":"start"}"#),
            (Chunk::StartStep, r#"{"type":"start-step"}"#),
            (Chunk::FinishStep, r#"{"type":"finish-step"}"#),
            (Chunk::Finish, r#"{"type":"finish"}"#),
            (
                Chunk::TextStart { id: "t0".into() },
                r#"{"type":"text-start","id":"t0"}"#,
            ),
            (
                Chunk::TextDelta {
                    id: "t0".into(),
                    delta: "hi".into(),
                },
                r#"{"type":"text-delta","id":"t0","delta":"hi"}"#,
            ),
            (
                Chunk::TextEnd { id: "t0".into() },
                r#"{"type":"text-end","id":"t0"}"#,
            ),
            (
                Chunk::ReasoningDelta {
                    id: "r0".into(),
                    delta: "hmm".into(),
                },
                r#"{"type":"reasoning-delta","id":"r0","delta":"hmm"}"#,
            ),
            (
                Chunk::ToolInputAvailable {
                    tool_call_id: "call_1".into(),
                    tool_name: "fs_read".into(),
                    input: serde_json::json!({"path": "/tmp/a"}),
                },
                r#"{"type":"tool-input-available","toolCallId":"call_1","toolName":"fs_read","input":{"path":"/tmp/a"}}"#,
            ),
            (
                Chunk::ToolOutputAvailable {
                    tool_call_id: "call_1".into(),
                    output: serde_json::json!({"ok": true}),
                },
                r#"{"type":"tool-output-available","toolCallId":"call_1","output":{"ok":true}}"#,
            ),
            (
                Chunk::ToolOutputError {
                    tool_call_id: "call_1".into(),
                    error_text: "boom".into(),
                },
                r#"{"type":"tool-output-error","toolCallId":"call_1","errorText":"boom"}"#,
            ),
            (
                Chunk::ToolApprovalRequest {
                    approval_id: "ap1".into(),
                    tool_call_id: "call_9".into(),
                },
                r#"{"type":"tool-approval-request","approvalId":"ap1","toolCallId":"call_9"}"#,
            ),
            (
                Chunk::ToolOutputDenied {
                    tool_call_id: "call_9".into(),
                },
                r#"{"type":"tool-output-denied","toolCallId":"call_9"}"#,
            ),
            (
                Chunk::Data {
                    name: "weather".into(),
                    id: "w0".into(),
                    data: serde_json::json!({"c": 21}),
                },
                r#"{"type":"data-weather","id":"w0","data":{"c":21}}"#,
            ),
            (
                // json-render inline mode: one RFC 6902 op per `data-spec` part.
                Chunk::Data {
                    name: "spec".into(),
                    id: "d0".into(),
                    data: serde_json::json!(
                        {"type": "patch", "patch": {"op": "add", "path": "/root", "value": "c1"}}
                    ),
                },
                r#"{"type":"data-spec","id":"d0","data":{"type":"patch","patch":{"op":"add","path":"/root","value":"c1"}}}"#,
            ),
            (
                Chunk::Error {
                    error_text: "x".into(),
                },
                r#"{"type":"error","errorText":"x"}"#,
            ),
        ];
        for (chunk, expected) in cases {
            let wire = chunk.to_wire();
            let a: Value = serde_json::from_str(&wire).unwrap();
            let b: Value = serde_json::from_str(expected).unwrap();
            assert_eq!(a, b, "chunk wire mismatch: {wire}");
        }
    }

    #[test]
    fn id_gen() {
        let mut g = IdGen::new("t");
        assert_eq!(g.next(), "t0");
        assert_eq!(g.next(), "t1");
    }
}
