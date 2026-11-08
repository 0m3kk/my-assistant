//! json-render inline mode — divert UI-spec patch lines out of the assistant
//! text stream into `data-spec` parts.
//!
//! Per the [`INLINE_SYSTEM_PROMPT`] contract, the model writes prose and, when
//! a visual answer helps, appends RFC 6902 JSON Patch operations — one per
//! line — that build a json-render spec. [`JsonRenderFilter`] sits between the
//! provider's text deltas and the stream collector:
//!
//! - prose streams through untouched (delta-sized, never held back),
//! - only a partial line that could still become a patch (starts with `{`/`[`)
//!   is buffered until the line completes,
//! - complete patch lines become [`TextEvent::Patch`] → `Chunk::Data` parts
//!   (`data-spec`, payload `{"type":"patch","patch":{...}}`), which the
//!   frontend merges via `@json-render`'s `useJsonRenderMessage`,
//! - fence markers around a patch block are swallowed (models habitually fence
//!   the JSONL despite instructions); fences around ordinary code pass through.
//!
//! The component catalog described in [`INLINE_SYSTEM_PROMPT`] is defined in
//! `frontend/src/lib/ai/widget-catalog.ts` — keep the two in sync.

use serde_json::Value;

/// A piece of assistant text output after json-render extraction.
#[derive(Debug, PartialEq)]
pub enum TextEvent {
    /// Plain prose — forward as text deltas.
    Prose { delta: String },
    /// A complete RFC 6902 op for the UI spec — emit as a `data-spec` part.
    Patch { op: Value },
}

/// System-prompt section teaching the model the inline json-render format.
/// Mirrors the frontend catalog (`frontend/src/lib/ai/widget-catalog.ts`).
pub const INLINE_SYSTEM_PROMPT: &str = r#"## Rich UI output (json-render)

When a visual presentation genuinely helps (metrics, trends, tables, structured comparisons), you may enrich your reply with a UI spec. Plain text answers stay plain text — never emit patches just to restate prose.

After (or between) your prose, emit RFC 6902 JSON Patch operations that build the UI. Exactly one operation per line, each line a complete JSON object, no code fences, nothing else on those lines:

{"op":"add","path":"/root","value":"card1"}
{"op":"add","path":"/elements/card1","value":{"type":"Card","props":{"title":"Revenue"},"children":["m1","c1"]}}

Rules:
- Add "/root" first; it references the id of the root element.
- Add every element at "/elements/<id>" with a unique <id>, a "type" from the catalog below, its "props", and a "children" array listing child element ids in display order (leaf elements use []).
- Never reference an id before or unless you add it.
- Numbers stay numbers in props (unquoted), e.g. {"values":[1,2,3]}.
- When a widget presents the data, do not repeat the same data as a markdown table or list in your prose — one sentence of context is enough.

Catalog:
- Card — titled container. props: {"title": string, "description": string?}. children allowed.
- Stack — layout container. props: {"direction": "vertical"|"horizontal"?, "gap": "sm"|"md"|"lg"?}. children allowed.
- Text — markdown text block. props: {"content": string}. leaf.
- Metric — single KPI. props: {"label": string, "value": string, "delta": string?}. leaf.
- Table — data table. props: {"headers": string[], "rows": (string|number)[][], "caption": string?}. leaf.
- Chart — bar/line/area chart. props: {"type": "bar"|"line"|"area", "title": string?, "categories": string[], "series": [{"name": string, "values": number[]}]}. Every series has exactly one value per category, in category order. leaf.

Example — "Show H1 revenue by quarter":

Revenue recovered strongly in Q2 — details below.

{"op":"add","path":"/root","value":"card1"}
{"op":"add","path":"/elements/card1","value":{"type":"Card","props":{"title":"Revenue H1"},"children":["m1","c1"]}}
{"op":"add","path":"/elements/m1","value":{"type":"Metric","props":{"label":"Total","value":"$1.2M","delta":"+12.4%"},"children":[]}}
{"op":"add","path":"/elements/c1","value":{"type":"Chart","props":{"type":"bar","categories":["Q1","Q2"],"series":[{"name":"Revenue","values":[500000,700000]}]},"children":[]}}"#;

const SPEC_PATH_PREFIXES: [&str; 3] = ["/root", "/elements", "/state"];
const RFC6902_OPS: [&str; 6] = ["add", "remove", "replace", "copy", "move", "test"];

#[derive(Debug, Default)]
pub struct JsonRenderFilter {
    buffer: String,
    /// A fence-open line (` ``` `) held back until we see whether the next
    /// line is a patch.
    pending_fence: Option<String>,
    /// Inside a fence whose first line was a patch — swallow fence markers,
    /// keep processing inner lines.
    in_patch_fence: bool,
}

impl JsonRenderFilter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one provider text delta; returns the events to emit. Prose is
    /// streamed through immediately; only a partial line that could still
    /// become a patch is buffered.
    pub fn feed(&mut self, delta: &str) -> Vec<TextEvent> {
        self.buffer.push_str(delta);
        let mut events = Vec::new();
        while let Some(pos) = self.buffer.find('\n') {
            let line: String = self.buffer.drain(..=pos).collect();
            let line = line.trim_end_matches(['\n', '\r']);
            self.process_line(line, &mut events);
        }
        // A partial line can only be held while it might still become a patch
        // (or while a fence decision is pending); everything else streams on.
        if !self.buffer.is_empty() && self.pending_fence.is_none() && !starts_json(&self.buffer) {
            let rest = std::mem::take(&mut self.buffer);
            events.push(TextEvent::Prose { delta: rest });
        }
        events
    }

    /// Flush at end of stream: the buffered partial line is classified as-is.
    pub fn flush(&mut self) -> Vec<TextEvent> {
        let mut events = Vec::new();
        let rest = std::mem::take(&mut self.buffer);
        if !rest.is_empty() {
            let line = rest.trim_end_matches('\r');
            classify_line(line, &mut events);
        }
        if let Some(fence) = self.pending_fence.take() {
            events.push(TextEvent::Prose {
                delta: format!("{fence}\n"),
            });
        }
        events
    }

    fn process_line(&mut self, line: &str, events: &mut Vec<TextEvent>) {
        let trimmed = line.trim();

        if self.in_patch_fence {
            if trimmed.starts_with("```") {
                self.in_patch_fence = false;
            } else {
                classify_line(line, events);
            }
            return;
        }

        if let Some(fence) = self.pending_fence.take() {
            if extract_patches(trimmed).is_some() {
                // The held fence opened a patch block: drop the fence markers.
                self.in_patch_fence = true;
                classify_line(line, events);
            } else {
                events.push(TextEvent::Prose {
                    delta: format!("{fence}\n"),
                });
                classify_line(line, events);
            }
            return;
        }

        if trimmed.starts_with("```") {
            self.pending_fence = Some(line.to_string());
            return;
        }

        classify_line(line, events);
    }
}

/// Classify one complete line: spec patches, or prose passed through.
fn classify_line(line: &str, events: &mut Vec<TextEvent>) {
    match extract_patches(line.trim()) {
        Some(ops) => {
            for op in ops {
                events.push(TextEvent::Patch { op });
            }
        }
        None => events.push(TextEvent::Prose {
            delta: format!("{line}\n"),
        }),
    }
}

/// Parse a line into RFC 6902 ops for the UI spec. Returns `None` when the
/// line is not purely spec patches — it then flows through as prose.
fn extract_patches(line: &str) -> Option<Vec<Value>> {
    if !line.starts_with('{') && !line.starts_with('[') {
        return None;
    }
    let value: Value = serde_json::from_str(line).ok()?;
    match value {
        Value::Object(_) => is_spec_op(&value).then_some(vec![value]),
        // Tolerate a whole patch array on one line by splitting into parts.
        Value::Array(items) if !items.is_empty() && items.iter().all(is_spec_op) => Some(items),
        _ => None,
    }
}

/// Does this look like an RFC 6902 op aimed at the json-render spec tree?
fn is_spec_op(v: &Value) -> bool {
    let Some(op) = v.get("op").and_then(Value::as_str) else {
        return false;
    };
    if !RFC6902_OPS.contains(&op) {
        return false;
    }
    let Some(path) = v.get("path").and_then(Value::as_str) else {
        return false;
    };
    SPEC_PATH_PREFIXES.iter().any(|p| path.starts_with(p))
}

/// Could this (partial) line still become a JSON patch line?
fn starts_json(s: &str) -> bool {
    let t = s.trim_start();
    t.starts_with('{') || t.starts_with('[')
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn patch_events(ops: Value) -> Vec<TextEvent> {
        match ops {
            Value::Array(items) => items
                .into_iter()
                .map(|op| TextEvent::Patch { op })
                .collect(),
            op => vec![TextEvent::Patch { op }],
        }
    }

    fn feed_all(filter: &mut JsonRenderFilter, input: &str) -> Vec<TextEvent> {
        let mut events = filter.feed(input);
        events.extend(filter.flush());
        events
    }

    #[test]
    fn prose_streams_incrementally() {
        let mut f = JsonRenderFilter::new();
        assert_eq!(
            f.feed("Hello wor"),
            vec![TextEvent::Prose {
                delta: "Hello wor".into()
            }]
        );
        assert_eq!(
            f.feed("ld\n"),
            vec![TextEvent::Prose {
                delta: "ld\n".into()
            }]
        );
    }

    #[test]
    fn patch_line_extracted() {
        let mut f = JsonRenderFilter::new();
        let line = r#"{"op":"add","path":"/root","value":"c1"}"#;
        assert_eq!(
            feed_all(&mut f, &format!("{line}\n")),
            patch_events(json!({"op":"add","path":"/root","value":"c1"}))
        );
    }

    #[test]
    fn patch_across_deltas() {
        let mut f = JsonRenderFilter::new();
        assert!(f.feed(r#"{"op":"add","pa"#).is_empty()); // held — may become a patch
        let events = f.feed(concat!(r#"th":"/root","value":"c1"}"#, "\n"));
        assert_eq!(
            events,
            patch_events(json!({"op":"add","path":"/root","value":"c1"}))
        );
    }

    #[test]
    fn prose_then_patch_in_one_delta() {
        let mut f = JsonRenderFilter::new();
        let events = f.feed("Hello\n{\"op\":\"add\",\"path\":\"/root\",\"value\":\"c1\"}\n");
        assert_eq!(
            events,
            vec![
                TextEvent::Prose {
                    delta: "Hello\n".into()
                },
                TextEvent::Patch {
                    op: json!({"op":"add","path":"/root","value":"c1"})
                },
            ]
        );
    }

    #[test]
    fn non_patch_json_is_prose() {
        let mut f = JsonRenderFilter::new();
        assert_eq!(
            feed_all(&mut f, "{\"foo\": 1}\n"),
            vec![TextEvent::Prose {
                delta: "{\"foo\": 1}\n".into()
            }]
        );
    }

    #[test]
    fn patch_with_wrong_path_is_prose() {
        let mut f = JsonRenderFilter::new();
        assert_eq!(
            feed_all(
                &mut f,
                "{\"op\":\"add\",\"path\":\"/users/1\",\"value\":\"x\"}\n"
            ),
            vec![TextEvent::Prose {
                delta: "{\"op\":\"add\",\"path\":\"/users/1\",\"value\":\"x\"}\n".into()
            }]
        );
    }

    #[test]
    fn array_of_ops_splits_into_patches() {
        let mut f = JsonRenderFilter::new();
        let line = r#"[{"op":"add","path":"/root","value":"c1"},{"op":"add","path":"/elements/c1","value":{"type":"Text","props":{"content":"hi"},"children":[]}}]"#;
        let events = feed_all(&mut f, &format!("{line}\n"));
        assert_eq!(events.len(), 2);
        assert!(events.iter().all(|e| matches!(e, TextEvent::Patch { .. })));
    }

    #[test]
    fn fenced_patches_are_unwrapped() {
        let mut f = JsonRenderFilter::new();
        assert!(f.feed("```json\n").is_empty()); // fence held, not yet decided
        let events = f.feed("{\"op\":\"add\",\"path\":\"/root\",\"value\":\"c1\"}\n");
        assert_eq!(
            events,
            patch_events(json!({"op":"add","path":"/root","value":"c1"}))
        );
        assert!(f.feed("```\n").is_empty()); // closing fence swallowed
        assert_eq!(
            f.feed("after\n"),
            vec![TextEvent::Prose {
                delta: "after\n".into()
            }]
        );
    }

    #[test]
    fn ordinary_code_fence_passes_through() {
        let mut f = JsonRenderFilter::new();
        assert!(f.feed("```python\n").is_empty()); // held one line
        assert_eq!(
            f.feed("print(1)\n"),
            vec![
                TextEvent::Prose {
                    delta: "```python\n".into()
                },
                TextEvent::Prose {
                    delta: "print(1)\n".into()
                },
            ]
        );
        assert!(f.feed("```\n").is_empty()); // held again — could open a patch fence
        assert_eq!(
            feed_all(&mut f, "done\n"),
            vec![
                TextEvent::Prose {
                    delta: "```\n".into()
                },
                TextEvent::Prose {
                    delta: "done\n".into()
                },
            ]
        );
    }

    #[test]
    fn unclosed_fence_flushes_as_prose() {
        let mut f = JsonRenderFilter::new();
        assert!(f.feed("```json\n").is_empty());
        assert_eq!(
            feed_all(&mut f, "{\"foo\": 1}"),
            vec![
                TextEvent::Prose {
                    delta: "{\"foo\": 1}\n".into()
                },
                TextEvent::Prose {
                    delta: "```json\n".into()
                },
            ]
        );
    }

    #[test]
    fn complete_patch_without_newline_flushes_as_patch() {
        let mut f = JsonRenderFilter::new();
        assert!(
            f.feed("{\"op\":\"add\",\"path\":\"/root\",\"value\":\"c1\"}")
                .is_empty()
        );
        assert_eq!(
            f.flush(),
            patch_events(json!({"op":"add","path":"/root","value":"c1"}))
        );
    }

    #[test]
    fn blank_and_crlf_lines() {
        let mut f = JsonRenderFilter::new();
        assert_eq!(
            feed_all(&mut f, "prose\r\n\r\n"),
            vec![
                TextEvent::Prose {
                    delta: "prose\n".into()
                },
                TextEvent::Prose { delta: "\n".into() },
            ]
        );
    }
}
