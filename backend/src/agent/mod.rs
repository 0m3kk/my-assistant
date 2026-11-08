//! Agent runtime: the tool-calling loop, history assembly from stored UI
//! message parts, tool registry assembly, and the approval state machine.

use std::sync::Arc;

use serde_json::{Value, json};
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::aistream::{Chunk, IdGen};
use crate::domain::{Agent, Model};
use crate::error::AppResult;
use crate::llm::{LlmMessage, Provider, ProviderCredentials, StreamCallbacks, ToolCallReq};
use crate::repo;
use crate::tools::{
    RegisteredTool, ToolError, ToolExecContext, ToolRegistry, ToolRuntime, builtin, file::FileTool,
    http_tool::HttpTool, merge_config, meta, wasm::WasmTool,
};

mod jsonrender;
use jsonrender::{INLINE_SYSTEM_PROMPT, JsonRenderFilter, TextEvent};

/// Identity of the run being executed (the agent and model it starts with live
/// in [`RunAgents`], which a switch can replace mid-run).
pub struct PreparedRun {
    pub run_id: Uuid,
    /// Resolved reasoning effort for this run (`low|medium|high`); None = unset.
    pub reasoning_effort: Option<String>,
}

/// The agent currently driving the turn, plus everything tied to it.
///
/// A `switch_agent` call swaps the active agent mid-run: the next model call
/// uses the new agent's prompt, model, tool registry and credentials, while the
/// same collector keeps streaming into one assistant message. `home` is the
/// agent the conversation belongs to (always `general` in chat).
pub struct RunAgents {
    pub active: Agent,
    pub home: Agent,
    pub model: Model,
    pub provider: Arc<dyn Provider>,
    pub credentials: ProviderCredentials,
    pub registry: ToolRegistry,
    /// Number of switches performed; capped at 1 so a specialist cannot switch.
    pub switch_depth: u8,
}

impl RunAgents {
    /// Resolve `slug` and make it the active agent. Rebuilds the tool registry
    /// and context for it, so its tools and approvals work exactly as they do
    /// for the surface agent.
    pub async fn switch_to(
        &mut self,
        ctx: &mut ToolExecContext,
        slug: &str,
        collector: &mut RunCollector,
    ) -> AppResult<()> {
        let target = repo::agents::by_slug(&ctx.pool, slug)
            .await?
            .filter(|a| a.status == "enabled")
            .ok_or_else(|| crate::error::AppError::not_found(format!("agent '{slug}' not found")))?;

        let model =
            crate::service::chat::resolve_model_pool(&ctx.pool, None, None, &target).await?;
        let (provider, credentials) = crate::service::chat::provider_credentials_with(
            &ctx.pool,
            &ctx.secrets,
            &ctx.providers,
            &model,
        )
        .await?;
        let registry = build_registry(
            &ctx.pool,
            target.id,
            ctx.searxng_url.as_deref(),
            ctx.default_tool_timeout_ms,
            ctx.default_tool_output_cap,
        )
        .await?;
        let assignments = repo::agents::tools_for_agent(&ctx.pool, target.id).await?;

        ctx.agent_id = target.id;
        ctx.agent_slug = target.slug.clone();
        ctx.allowed_paths = allowed_paths_from(&assignments);
        ctx.switch_depth += 1;

        // The specialist gets the task as its instruction. Previous turns stay
        // in `history` so it still sees the conversation.
        collector.emit(Chunk::Data {
            name: "switch".into(),
            id: format!("sw{}", ctx.switch_depth),
            data: json!({
                "to": target.slug,
                "toName": target.name,
                "status": "active",
            }),
        });

        self.active = target;
        self.model = model;
        self.provider = provider;
        self.credentials = credentials;
        self.registry = registry;
        self.switch_depth += 1;
        Ok(())
    }

    /// Emit the "returned" marker so the transcript records the hand-back and
    /// the next request starts on `home` again.
    pub fn mark_returned(&self, ctx: &ToolExecContext, collector: &mut RunCollector) {
        collector.emit(Chunk::Data {
            name: "switch".into(),
            id: format!("sw{}-back", ctx.switch_depth),
            data: json!({
                "to": self.home.slug,
                "toName": self.home.name,
                "status": "returned",
            }),
        });
    }
}

/// Slug of the agent a stored transcript is currently switched to, if any.
///
/// A switch that ended in an approval request leaves a trailing `data-switch`
/// part with `status: "active"` — the next HTTP request must resume under that
/// agent, otherwise it would run the specialist's tool with the surface agent's
/// registry and fail.
pub fn active_agent_slug(parts: &[Value]) -> Option<String> {
    parts
        .iter()
        .filter(|p| p.get("type").and_then(Value::as_str) == Some("data-switch"))
        .filter_map(|p| p.get("data"))
        .filter(|d| d.get("status").and_then(Value::as_str) == Some("active"))
        .filter_map(|d| d.get("to").and_then(Value::as_str))
        .next_back()
        .map(str::to_string)
}

/// Load `slug` into `agents` before a resumed request runs.
///
/// Returns `false` when the agent is gone or disabled, in which case the caller
/// keeps the surface agent (the user's pending approval can no longer be
/// honoured, so the run simply continues on the home agent).
pub async fn resume_switched_agent(
    state: &crate::state::AppState,
    ctx: &mut ToolExecContext,
    slug: &str,
    agents: &mut RunAgents,
) -> AppResult<bool> {
    let Some(target) = repo::agents::by_slug(&state.pool, slug)
        .await?
        .filter(|a| a.status == "enabled")
    else {
        return Ok(false);
    };

    let model =
        crate::service::chat::resolve_model_pool(&state.pool, None, None, &target).await?;
    let (provider, credentials) = crate::service::chat::provider_credentials_with(
        &state.pool,
        &state.secrets,
        &state.providers,
        &model,
    )
    .await?;
    let registry = build_registry(
        &state.pool,
        target.id,
        ctx.searxng_url.as_deref(),
        ctx.default_tool_timeout_ms,
        ctx.default_tool_output_cap,
    )
    .await?;
    let assignments = repo::agents::tools_for_agent(&state.pool, target.id).await?;

    ctx.agent_id = target.id;
    ctx.agent_slug = target.slug.clone();
    ctx.allowed_paths = allowed_paths_from(&assignments);

    agents.active = target;
    agents.model = model;
    agents.provider = provider;
    agents.credentials = credentials;
    agents.registry = registry;
    Ok(true)
}

/// A tool call from a previous turn that the user approved and is now resumed.
pub struct ResumeCall {
    pub tool_call_id: String,
    pub tool_name: String,
    pub input: Value,
    pub stored_message_id: Uuid,
}

/// Outcome of a finished run — used by the caller to persist state.
pub struct RunStatus {
    /// Sum of prompt tokens across every provider call in this run.
    pub usage_prompt: Option<i64>,
    /// Sum of completion tokens across every provider call in this run.
    pub usage_completion: Option<i64>,
    /// Prompt tokens of the *final* call — i.e. how full the context window
    /// was when the model last ran. This is what a context meter shows.
    pub context_tokens: Option<i64>,
    pub approval_pending: bool,
    pub cancelled: bool,
}

/// Emits chunks and mirrors them into the assistant `parts` accumulator.
pub struct RunCollector {
    pub tx: tokio::sync::mpsc::UnboundedSender<Chunk>,
    pub parts: Vec<Value>,
    text_gen: IdGen,
    reasoning_gen: IdGen,
    data_gen: IdGen,
    /// json-render inline-mode filter: diverts UI-spec patch lines from the
    /// text stream into `data-spec` parts (see [`jsonrender`]).
    jr_filter: JsonRenderFilter,
    active_text_id: Option<String>,
    active_reasoning_id: Option<String>,
    disconnected: bool,
    /// Optional periodic snapshot sink: lets the caller persist the partial
    /// assistant message while the run is still going, so clients that poll
    /// (reload / another tab) see progress.
    snapshot_tx: Option<tokio::sync::watch::Sender<Vec<Value>>>,
    last_snapshot: std::time::Instant,
    /// Replayable stream for SSE resume (clients that reconnect after reload).
    stream: Option<Arc<crate::service::run_stream::RunStream>>,
}

/// Minimum gap between two partial-message snapshots.
const SNAPSHOT_INTERVAL: std::time::Duration = std::time::Duration::from_millis(750);

impl RunCollector {
    pub fn new(tx: tokio::sync::mpsc::UnboundedSender<Chunk>) -> Self {
        Self {
            tx,
            parts: Vec::new(),
            text_gen: IdGen::new("t"),
            reasoning_gen: IdGen::new("r"),
            data_gen: IdGen::new("d"),
            jr_filter: JsonRenderFilter::new(),
            active_text_id: None,
            active_reasoning_id: None,
            disconnected: false,
            snapshot_tx: None,
            last_snapshot: std::time::Instant::now() - SNAPSHOT_INTERVAL,
            stream: None,
        }
    }

    /// Attach a sink that receives throttled copies of the accumulated parts.
    pub fn with_snapshot(mut self, tx: tokio::sync::watch::Sender<Vec<Value>>) -> Self {
        self.snapshot_tx = Some(tx);
        self
    }

    /// Attach the replayable stream used by `GET /api/chat/resume`.
    pub fn with_stream(mut self, stream: Arc<crate::service::run_stream::RunStream>) -> Self {
        self.stream = Some(stream);
        self
    }

    /// Force a snapshot (call once when the run ends).
    pub fn flush_snapshot(&mut self) {
        if let Some(tx) = &self.snapshot_tx {
            let _ = tx.send(self.parts.clone());
        }
    }

    fn maybe_snapshot(&mut self) {
        let Some(tx) = &self.snapshot_tx else {
            return;
        };
        if self.last_snapshot.elapsed() < SNAPSHOT_INTERVAL {
            return;
        }
        self.last_snapshot = std::time::Instant::now();
        let _ = tx.send(self.parts.clone());
    }

    #[allow(dead_code)] // retained for diagnostics; disconnects no longer stop runs
    pub fn is_disconnected(&self) -> bool {
        self.disconnected
    }

    pub fn emit(&mut self, chunk: Chunk) {
        self.mirror_to_parts(&chunk);
        if let Some(stream) = &self.stream {
            stream.publish(chunk.clone());
        }
        if !self.disconnected && self.tx.send(chunk).is_err() {
            self.disconnected = true; // client went away — keep running to persist
        }
        self.maybe_snapshot();
    }

    fn mirror_to_parts(&mut self, chunk: &Chunk) {
        match chunk {
            Chunk::TextStart { id } => {
                self.active_text_id = Some(id.clone());
                self.parts.push(json!({"type": "text", "text": ""}));
            }
            Chunk::TextDelta { delta, .. } => {
                if let Some(part) = self
                    .parts
                    .iter_mut()
                    .rev()
                    .find(|p| p.get("type").and_then(Value::as_str) == Some("text"))
                {
                    let current = part
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    part["text"] = json!(format!("{current}{delta}"));
                }
            }
            Chunk::TextEnd { .. } => {
                self.active_text_id = None;
            }
            Chunk::ReasoningStart { id } => {
                self.active_reasoning_id = Some(id.clone());
                self.parts.push(json!({"type": "reasoning", "text": ""}));
            }
            Chunk::ReasoningDelta { delta, .. } => {
                if let Some(part) = self
                    .parts
                    .iter_mut()
                    .rev()
                    .find(|p| p.get("type").and_then(Value::as_str) == Some("reasoning"))
                {
                    let current = part
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    part["text"] = json!(format!("{current}{delta}"));
                }
            }
            Chunk::ReasoningEnd { .. } => {
                self.active_reasoning_id = None;
            }
            Chunk::ToolInputAvailable {
                tool_call_id,
                tool_name,
                input,
            } => {
                self.parts.push(json!({
                    "type": format!("tool-{tool_name}"),
                    "toolCallId": tool_call_id,
                    "state": "input-available",
                    "input": input,
                }));
            }
            Chunk::ToolOutputAvailable {
                tool_call_id,
                output,
            } => {
                if let Some(part) = self.tool_part_mut(tool_call_id) {
                    part["state"] = json!("output-available");
                    part["output"] = output.clone();
                }
            }
            Chunk::ToolOutputError {
                tool_call_id,
                error_text,
            } => {
                if let Some(part) = self.tool_part_mut(tool_call_id) {
                    part["state"] = json!("output-error");
                    part["errorText"] = json!(error_text);
                }
            }
            Chunk::ToolApprovalRequest {
                approval_id,
                tool_call_id,
            } => {
                if let Some(part) = self.tool_part_mut(tool_call_id) {
                    part["state"] = json!("approval-requested");
                    part["approval"] = json!({"id": approval_id});
                }
            }
            Chunk::ToolOutputDenied { tool_call_id } => {
                if let Some(part) = self.tool_part_mut(tool_call_id) {
                    part["state"] = json!("output-denied");
                }
            }
            Chunk::Data { name, id, data } if name != "conversation" => {
                // Usage is re-emitted every step with cumulative totals, so it
                // replaces the existing part instead of stacking duplicates.
                if name == "usage" {
                    match self
                        .parts
                        .iter_mut()
                        .find(|p| p.get("type").and_then(Value::as_str) == Some("data-usage"))
                    {
                        Some(part) => part["data"] = data.clone(),
                        None => self.parts.push(json!({
                            "type": "data-usage", "id": id, "data": data
                        })),
                    }
                } else {
                    self.parts
                        .push(json!({"type": format!("data-{name}"), "id": id, "data": data}));
                }
            }
            _ => {}
        }
    }

    fn tool_part_mut(&mut self, tool_call_id: &str) -> Option<&mut Value> {
        self.parts.iter_mut().find(|p| {
            p.get("type")
                .and_then(Value::as_str)
                .is_some_and(|t| t.starts_with("tool-"))
                && p.get("toolCallId").and_then(Value::as_str) == Some(tool_call_id)
        })
    }

    pub fn next_data_id(&mut self) -> String {
        self.data_gen.next()
    }

    /// Feed an assistant text delta through the json-render filter: prose
    /// streams as text deltas; UI-spec patch lines become `data-spec` parts.
    pub fn feed_text(&mut self, delta: &str) {
        for event in self.jr_filter.feed(delta) {
            self.emit_text_event(event);
        }
    }

    /// Flush the json-render filter at end of stream (buffered partial line).
    pub fn flush_text(&mut self) {
        for event in self.jr_filter.flush() {
            self.emit_text_event(event);
        }
    }

    fn emit_text_event(&mut self, event: TextEvent) {
        match event {
            TextEvent::Prose { delta } => {
                self.begin_text_if_needed();
                let id = self.active_text_id().to_string();
                self.emit(Chunk::TextDelta { id, delta });
            }
            TextEvent::Patch { op } => {
                // Close the running text part so later prose opens a fresh
                // part ordered after the spec parts on the client.
                self.end_text_if_open();
                let id = self.next_data_id();
                self.emit(Chunk::Data {
                    name: "spec".into(),
                    id,
                    data: json!({"type": "patch", "patch": op}),
                });
            }
        }
    }

    /// Start a text part if none is open.
    pub fn begin_text_if_needed(&mut self) {
        if self.active_text_id.is_none() {
            let id = self.text_gen.next();
            self.emit(Chunk::TextStart { id });
        }
    }

    pub fn active_text_id(&self) -> &str {
        self.active_text_id.as_deref().unwrap_or("")
    }

    pub fn active_reasoning_id(&self) -> &str {
        self.active_reasoning_id.as_deref().unwrap_or("")
    }

    /// Start a reasoning part if none is open.
    pub fn begin_reasoning_if_needed(&mut self) {
        if self.active_reasoning_id.is_none() {
            let id = self.reasoning_gen.next();
            self.emit(Chunk::ReasoningStart { id });
        }
    }

    pub fn end_text_if_open(&mut self) {
        if let Some(id) = self.active_text_id.take() {
            self.emit(Chunk::TextEnd { id });
        }
    }

    pub fn reasoning_open(&self) -> bool {
        self.active_reasoning_id.is_some()
    }

    fn end_reasoning_if_open(&mut self) {
        if let Some(id) = self.active_reasoning_id.take() {
            self.emit(Chunk::ReasoningEnd { id });
        }
    }
}

/// Bridge from provider callbacks into the collector.
struct CollectorCallbacks<'a> {
    collector: &'a mut RunCollector,
}

impl StreamCallbacks for CollectorCallbacks<'_> {
    fn on_text_delta(&mut self, delta: &str) {
        self.collector.feed_text(delta);
    }

    fn on_reasoning_delta(&mut self, delta: &str) {
        self.collector.begin_reasoning_if_needed();
        let id = self.collector.active_reasoning_id().to_string();
        self.collector.emit(Chunk::ReasoningDelta {
            id,
            delta: delta.to_string(),
        });
    }

    fn on_tool_call(&mut self, call: &ToolCallReq) {
        self.collector.end_text_if_open();
        self.collector.emit(Chunk::ToolInputAvailable {
            tool_call_id: call.id.clone(),
            tool_name: call.name.clone(),
            input: call.arguments.clone(),
        });
    }
}

enum ToolExecOutcome {
    Done,
    ApprovalPending,
}

/// Everything one run needs to start: the conversation state it resumes from.
pub struct RunInput {
    pub history: Vec<LlmMessage>,
    /// Approved tool calls from the previous turn, executed before the loop.
    pub resume_calls: Vec<ResumeCall>,
    /// Denied tool calls from the previous turn, mirrored to the client.
    pub denied_calls: Vec<(Uuid, String)>,
}

/// Execute one full agent turn, streaming chunks into `collector`.
pub async fn execute_run(
    prepared: &PreparedRun,
    agents: &mut RunAgents,
    ctx: &mut ToolExecContext,
    input: RunInput,
    collector: &mut RunCollector,
    token: &CancellationToken,
) -> RunStatus {
    let RunInput {
        mut history,
        resume_calls,
        denied_calls,
    } = input;
    let mut status = RunStatus {
        usage_prompt: None,
        usage_completion: None,
        context_tokens: None,
        approval_pending: false,
        cancelled: false,
    };

    collector.emit(Chunk::Start);

    // Stable run identity for feedback (persisted as `data-run`).
    collector.emit(Chunk::Data {
        name: "run".into(),
        id: "meta".into(),
        data: json!({
            "runId": prepared.run_id,
            "agentId": agents.active.id,
            "agentSlug": agents.active.slug,
            "modelId": agents.model.id,
        }),
    });

    // 0. Mirror previously-denied tool calls so the client state stays aligned.
    for (stored_message_id, tool_call_id) in denied_calls {
        let _ = repo::conversations::update_tool_part(
            &ctx.pool,
            stored_message_id,
            &tool_call_id,
            "output-denied",
            None,
            None,
        )
        .await;
        collector.emit(Chunk::ToolOutputDenied { tool_call_id });
    }

    // 1. Execute tools approved in a previous turn (resume) and stream outputs.
    for call in resume_calls {
        let outcome = ToolRuntime::execute(
            &agents.registry,
            ctx,
            &call.tool_call_id,
            &call.tool_name,
            call.input.clone(),
        )
        .await;
        let (state, output, error_text, result_content) = match outcome {
            Ok(output) => (
                "output-available",
                Some(output.clone()),
                None,
                serde_json::to_string(&output).unwrap_or_default(),
            ),
            Err(ToolError::Message(e)) => (
                "output-error",
                None,
                Some(e.clone()),
                format!("Tool error: {e}"),
            ),
            Err(ToolError::OutOfRoot(path)) => (
                "output-error",
                None,
                Some(format!("out of root: {path}")),
                format!("Tool error: out of root: {path}"),
            ),
        };
        // Mirror into the stored message from the previous turn.
        let _ = repo::conversations::update_tool_part(
            &ctx.pool,
            call.stored_message_id,
            &call.tool_call_id,
            state,
            output.clone(),
            error_text.as_deref(),
        )
        .await;
        collector.emit(match state {
            "output-available" => Chunk::ToolOutputAvailable {
                tool_call_id: call.tool_call_id.clone(),
                output: output.unwrap_or(json!({})),
            },
            _ => Chunk::ToolOutputError {
                tool_call_id: call.tool_call_id.clone(),
                error_text: error_text.unwrap_or_default(),
            },
        });
        history.push(LlmMessage::ToolResult {
            tool_call_id: call.tool_call_id,
            content: result_content,
        });
    }

    // A previous request may have ended mid-switch (an approval request from
    // the specialist). Its `data-switch` part tells us which agent must resume.
    // `switch_to` is best-effort here: the caller already had the chance to
    // restore the agent before the run started.
    if let Some(slug) = active_agent_slug(&collector.parts) {
        if slug != agents.active.slug {
            let _ = agents.switch_to(ctx, &slug, collector).await;
        }
    }

    let max_tool_calls = agents.active.max_tool_calls.max(0) as usize;
    let mut tool_call_count = 0usize;

    'outer: loop {
        // Client disconnects do NOT stop the run: the reply completes and is
        // persisted (switching conversations or closing the tab is safe).
        // Cancellation is explicit via the token (Stop button / cancel API).
        if token.is_cancelled() {
            status.cancelled = true;
            break 'outer;
        }

        collector.emit(Chunk::StartStep);

        let mut messages = Vec::with_capacity(history.len() + 1);
        // Agent system prompt + the json-render inline contract (UI widgets).
        let mut system = agents.active.system_prompt.clone();
        if !system.is_empty() {
            system.push_str("\n\n");
        }
        system.push_str(INLINE_SYSTEM_PROMPT);
        messages.push(LlmMessage::System { content: system });
        messages.extend(history.iter().cloned());

        let tool_defs = agents.registry.definitions();
        let mut callbacks = CollectorCallbacks { collector };
        let stream = agents.provider.stream_chat(
            Uuid::new_v4(),
            &agents.model.api_name,
            &messages,
            &tool_defs,
            agents.active.temperature,
            agents.active.max_tokens,
            prepared.reasoning_effort.as_deref(),
            &agents.credentials,
            &mut callbacks,
        );
        let selected = tokio::select! {
            biased;
            _ = token.cancelled() => None,
            r = stream => Some(r),
        };
        let reasoning_open = callbacks.collector.reasoning_open();
        // Ends the &mut borrow of `collector` before it is used again below.
        // (drop is deliberate: the borrow, not a destructor, is what matters.)
        #[allow(clippy::drop_non_drop)]
        drop(callbacks);

        let outcome = match selected {
            None => {
                status.cancelled = true;
                break 'outer;
            }
            Some(Err(e)) => {
                collector.flush_text();
                collector.end_text_if_open();
                collector.emit(Chunk::FinishStep);
                collector.emit(Chunk::Error {
                    error_text: e.to_string(),
                });
                break 'outer;
            }
            Some(Ok(o)) => o,
        };

        if reasoning_open {
            collector.end_reasoning_if_open();
        }
        collector.flush_text();
        collector.end_text_if_open();

        // Usage accumulates across steps (total cost); `context_tokens` keeps
        // only the last call's prompt size — that is how full the context
        // window was when the model last ran.
        if let Some(p) = outcome.usage.prompt_tokens {
            status.usage_prompt = Some(status.usage_prompt.unwrap_or(0) + p);
            status.context_tokens = Some(p);
        }
        if let Some(c) = outcome.usage.completion_tokens {
            status.usage_completion = Some(status.usage_completion.unwrap_or(0) + c);
        }

        // Mirror usage to the client as a `data-usage` part so the composer's
        // context meter updates while the run is still streaming.
        if status.usage_prompt.is_some() || status.usage_completion.is_some() {
            collector.emit(Chunk::Data {
                name: "usage".into(),
                id: "meta".into(),
                data: json!({
                    "promptTokens": status.usage_prompt,
                    "completionTokens": status.usage_completion,
                    "contextTokens": status.context_tokens,
                    "contextWindow": agents.model.context_window,
                }),
            });
        }

        // The specialist finished its turn: hand control back to the surface
        // agent for the next message and stop here (its answer is the reply).
        if agents.switch_depth > 0 && outcome.tool_calls.is_empty() {
            agents.mark_returned(ctx, collector);
            collector.emit(Chunk::FinishStep);
            break 'outer;
        }

        if outcome.tool_calls.is_empty() || tool_call_count >= max_tool_calls {
            collector.emit(Chunk::FinishStep);
            break 'outer;
        }

        // Record the assistant turn (text + tool calls) into the history.
        history.push(LlmMessage::Assistant {
            content: if outcome.text.is_empty() {
                None
            } else {
                Some(outcome.text.clone())
            },
            tool_calls: outcome.tool_calls.clone(),
        });

        for call in outcome.tool_calls {
            tool_call_count += 1;

            // Engine-level tool: hand the turn to another agent. Intercepted
            // before the registry lookup so the switch happens instead of the
            // tool body, and the next model call runs as the new agent.
            let switch_target = agents
                .registry
                .get(&call.name)
                .and_then(|r| r.tool.switch_target(&call.arguments));
            if let Some(target) = switch_target {
                // Refuse when the agent is already switched (depth cap) or the
                // target is the current agent; surface a normal tool error.
                let rejection = if ctx.switch_depth >= 1 {
                    Some("agent switch depth exceeded — a specialist cannot switch again".to_string())
                } else if target == agents.active.slug {
                    Some("already running that agent".to_string())
                } else {
                    None
                };
                if let Some(msg) = rejection {
                    collector.emit(Chunk::ToolOutputError {
                        tool_call_id: call.id.clone(),
                        error_text: msg.clone(),
                    });
                    history.push(LlmMessage::ToolResult {
                        tool_call_id: call.id.clone(),
                        content: format!("Tool error: {msg}"),
                    });
                    continue;
                }

                // The task becomes the specialist's instruction, appended to
                // the conversation it already sees.
                let task = call
                    .arguments
                    .get("task")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                collector.emit(Chunk::ToolOutputAvailable {
                    tool_call_id: call.id.clone(),
                    output: json!({"switched_to": target}),
                });
                history.push(LlmMessage::ToolResult {
                    tool_call_id: call.id.clone(),
                    content: format!("Switched to agent '{target}'."),
                });
                if !task.is_empty() {
                    history.push(LlmMessage::User {
                        content: task,
                        images: Vec::new(),
                    });
                }

                if let Err(e) = agents.switch_to(ctx, &target, collector).await {
                    collector.emit(Chunk::ToolOutputError {
                        tool_call_id: call.id.clone(),
                        error_text: e.to_string(),
                    });
                    break 'outer;
                }
                collector.emit(Chunk::FinishStep);
                continue 'outer;
            }

            let Some(registered) = agents.registry.get(&call.name) else {
                let msg = format!("unknown tool: {}", call.name);
                collector.emit(Chunk::ToolOutputError {
                    tool_call_id: call.id.clone(),
                    error_text: msg.clone(),
                });
                history.push(LlmMessage::ToolResult {
                    tool_call_id: call.id,
                    content: msg,
                });
                continue;
            };

            if !registered.requires_approval {
                if matches!(
                    execute_tool_call(agents, ctx, &mut history, collector, &call).await,
                    ToolExecOutcome::ApprovalPending
                ) {
                    status.approval_pending = true;
                    collector.emit(Chunk::FinishStep);
                    break 'outer;
                }
                // A tool that hands work to a human (a change proposal) ends the
                // turn: the model must not keep going before it is reviewed.
                if registered.tool.ends_turn() {
                    // A proposal from a specialist is the end of the hand-off,
                    // so record the return; a pending approval is not (that
                    // request resumes under the specialist).
                    if agents.switch_depth > 0 {
                        agents.mark_returned(ctx, collector);
                    }
                    collector.emit(Chunk::FinishStep);
                    break 'outer;
                }
                continue;
            }

            let decision = repo::approvals::by_conversation_and_call(
                &ctx.pool,
                ctx.conversation_id,
                &call.id,
            )
            .await
            .ok()
            .flatten()
            .map(|a| a.status);

            match decision.as_deref() {
                Some("approved") => {
                    if matches!(
                        execute_tool_call(agents, ctx, &mut history, collector, &call).await,
                        ToolExecOutcome::ApprovalPending
                    ) {
                        status.approval_pending = true;
                        collector.emit(Chunk::FinishStep);
                        break 'outer;
                    }
                    if registered.tool.ends_turn() {
                        if agents.switch_depth > 0 {
                            agents.mark_returned(ctx, collector);
                        }
                        collector.emit(Chunk::FinishStep);
                        break 'outer;
                    }
                }
                Some("denied") => {
                    collector.emit(Chunk::ToolOutputDenied {
                        tool_call_id: call.id.clone(),
                    });
                    history.push(LlmMessage::ToolResult {
                        tool_call_id: call.id.clone(),
                        content: "The user denied this tool call.".into(),
                    });
                }
                _ => {
                    // First time this call surfaces: request approval, end the turn.
                    let approval_id = repo::approvals::create_pending(
                        &ctx.pool,
                        ctx.conversation_id,
                        &call.id,
                        &call.name,
                        call.arguments.clone(),
                        "requires_approval",
                        None,
                    )
                    .await
                    .map(|a| a.id.to_string())
                    .unwrap_or_default();
                    collector.emit(Chunk::ToolApprovalRequest {
                        approval_id,
                        tool_call_id: call.id.clone(),
                    });
                    history.push(LlmMessage::ToolResult {
                        tool_call_id: call.id,
                        content: "Awaiting user approval.".into(),
                    });
                    status.approval_pending = true;
                    collector.emit(Chunk::FinishStep);
                    break 'outer;
                }
            }
        }

        collector.emit(Chunk::FinishStep);

        if tool_call_count >= max_tool_calls {
            break 'outer;
        }
    }

    if status.cancelled {
        collector.emit(Chunk::Abort);
    }
    collector.emit(Chunk::Finish);

    status
}

/// Assemble the tool registry for one agent run from builtins + DB rows.
pub async fn build_registry(
    pool: &PgPool,
    agent_id: Uuid,
    searxng_url: Option<&str>,
    default_timeout_ms: u64,
    default_output_cap: usize,
) -> AppResult<ToolRegistry> {
    let mut registry = ToolRegistry::empty();
    let assignments = repo::agents::tools_for_agent(pool, agent_id).await?;

    for (assignment, row) in assignments {
        let config = merge_config(&row.config, assignment.config.as_ref());
        let timeout_ms = config
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(default_timeout_ms);
        let output_cap = config
            .get("max_output_bytes")
            .and_then(Value::as_u64)
            .map(|v| v as usize)
            .unwrap_or(default_output_cap);
        let requires_approval = assignment
            .requires_approval
            .unwrap_or(row.requires_approval);

        let tool: Arc<dyn crate::tools::Tool> = match (row.kind.as_str(), row.source_ref.as_str()) {
            ("builtin", "workspace_list") => Arc::new(crate::tools::fs::FsList),
            ("builtin", "workspace_read") => Arc::new(crate::tools::fs::FsRead),
            ("builtin", "workspace_write") => Arc::new(crate::tools::fs::FsWrite),
            ("builtin", "current_time") => Arc::new(builtin::CurrentTime),
            ("builtin", "web_search") => Arc::new(builtin::WebSearch::new(
                config.clone(),
                searxng_url.map(str::to_string),
            )),
            ("builtin", "save_artifact") => Arc::new(crate::tools::artifact::SaveArtifact),
            ("builtin", "switch_agent") => Arc::new(crate::tools::r#switch::SwitchAgent),
            ("builtin", "meta.list_tools") => Arc::new(meta::ListTools),
            ("builtin", "meta.get_tool") => Arc::new(meta::GetTool),
            ("builtin", "meta.list_agents") => Arc::new(meta::ListAgents),
            ("builtin", "meta.get_agent") => Arc::new(meta::GetAgent),
            ("builtin", "meta.list_feedback") => Arc::new(meta::ListFeedback),
            ("builtin", "meta.list_executions") => Arc::new(meta::ListExecutions),
            ("builtin", "meta.validate_tool") => Arc::new(meta::ValidateTool),
            ("builtin", "meta.test_tool") => Arc::new(meta::TestTool),
            ("builtin", "meta.propose_tool") => Arc::new(meta::ProposeTool),
            ("builtin", "meta.propose_agent") => Arc::new(meta::ProposeAgent),
            ("builtin", "meta.list_proposals") => Arc::new(meta::ListProposals),
            ("builtin", "meta.list_folders") => Arc::new(meta::ListFolders),
            ("http", _) => Arc::new(HttpTool {
                name: row.name.clone(),
                description: if row.description.is_empty() {
                    row.display_name.clone()
                } else {
                    row.description.clone()
                },
                parameters_schema: row.parameters_schema.clone(),
                config: config.clone(),
            }),
            ("file", _) => Arc::new(FileTool {
                name: row.name.clone(),
                description: if row.description.is_empty() {
                    row.display_name.clone()
                } else {
                    row.description.clone()
                },
                parameters_schema: row.parameters_schema.clone(),
                config: config.clone(),
            }),
            ("wasm", _) => Arc::new(WasmTool {
                name: row.name.clone(),
                description: if row.description.is_empty() {
                    row.display_name.clone()
                } else {
                    row.description.clone()
                },
                parameters_schema: row.parameters_schema.clone(),
                config: config.clone(),
            }),
            _ => continue, // native/mcp not wired yet
        };

        registry.insert(RegisteredTool {
            tool_id: row.id,
            kind: row.kind.clone(),
            config,
            tool,
            requires_approval,
            timeout: std::time::Duration::from_millis(timeout_ms),
            max_output_bytes: output_cap,
        });
    }

    Ok(registry)
}

/// Extract whitelisted paths configured on any assigned fs tool (local mode).
pub fn allowed_paths_from(
    rows: &[(crate::domain::AgentToolRow, crate::domain::ToolRow)],
) -> Vec<std::path::PathBuf> {
    rows.iter()
        .filter(|(_, t)| t.source_ref.starts_with("workspace_") || t.source_ref.starts_with("fs_"))
        .filter_map(|(a, _)| a.config.as_ref())
        .filter_map(|c| c.get("allowed_paths").and_then(Value::as_array).cloned())
        .flatten()
        .filter_map(|p| p.as_str().map(std::path::PathBuf::from))
        .collect()
}

/// Build the sandbox context path for a conversation.
pub fn sandbox_root_for(
    config: &crate::config::Config,
    conversation_id: Uuid,
) -> std::path::PathBuf {
    config.workspace_root().join(conversation_id.to_string())
}

/// Execute a tool call, stream its output/error chunk, and append the LLM result.
async fn execute_tool_call(
    agents: &RunAgents,
    ctx: &ToolExecContext,
    history: &mut Vec<LlmMessage>,
    collector: &mut RunCollector,
    call: &ToolCallReq,
) -> ToolExecOutcome {
    let result = ToolRuntime::execute(
        &agents.registry,
        ctx,
        &call.id,
        &call.name,
        call.arguments.clone(),
    )
    .await;
    match result {
        Ok(output) => {
            collector.emit(Chunk::ToolOutputAvailable {
                tool_call_id: call.id.clone(),
                output: output.clone(),
            });
            history.push(LlmMessage::ToolResult {
                tool_call_id: call.id.clone(),
                content: serde_json::to_string(&output).unwrap_or_default(),
            });
            ToolExecOutcome::Done
        }
        Err(ToolError::Message(e)) => {
            collector.emit(Chunk::ToolOutputError {
                tool_call_id: call.id.clone(),
                error_text: e.clone(),
            });
            history.push(LlmMessage::ToolResult {
                tool_call_id: call.id.clone(),
                content: format!("Tool error: {e}"),
            });
            ToolExecOutcome::Done
        }
        Err(ToolError::OutOfRoot(path)) => {
            let approval_id = repo::approvals::create_pending(
                &ctx.pool,
                ctx.conversation_id,
                &call.id,
                &call.name,
                call.arguments.clone(),
                "out_of_root",
                Some(&path),
            )
            .await
            .map(|a| a.id.to_string())
            .unwrap_or_default();
            collector.emit(Chunk::ToolApprovalRequest {
                approval_id,
                tool_call_id: call.id.clone(),
            });
            history.push(LlmMessage::ToolResult {
                tool_call_id: call.id.clone(),
                content: format!("Awaiting user approval to access out-of-root path: {path}"),
            });
            ToolExecOutcome::ApprovalPending
        }
    }
}

/// Convert stored message rows into LLM history + resume/denied calls.
///
/// `files`/`payloads` carry attachment metadata + pre-resolved inline content
/// (see `service::files::resolve_inline_payloads`): images become vision
/// content blocks (when `vision`), text-like files are inlined, anything else
/// degrades to a one-line note in the user turn.
pub type HistoryAssembly = (Vec<LlmMessage>, Vec<ResumeCall>, Vec<(Uuid, String)>);

pub fn history_from_messages(
    rows: &[crate::domain::MessageRow],
    files: &std::collections::HashMap<Uuid, crate::domain::FileRow>,
    payloads: &std::collections::HashMap<Uuid, crate::service::files::InlinePayload>,
    vision: bool,
) -> HistoryAssembly {
    use crate::domain::ui_message::UIMessage;
    use crate::service::files::InlinePayload;

    let mut history = Vec::new();
    let mut resume = Vec::new();
    let mut denied: Vec<(Uuid, String)> = Vec::new();

    for row in rows {
        let Ok(message) = serde_json::from_value::<UIMessage>(json!({
            "id": row.id.to_string(),
            "role": row.role,
            "parts": row.parts,
        })) else {
            continue;
        };

        if message.is_user() {
            let mut text = message.text();
            let mut images: Vec<String> = Vec::new();
            for fp in message.file_parts() {
                let Some(file_row) = files.get(&fp.file_id) else {
                    continue;
                };
                match payloads.get(&fp.file_id) {
                    Some(InlinePayload::ImageUrl(url)) if vision => {
                        images.push(url.clone());
                    }
                    Some(InlinePayload::Text(content)) => {
                        text.push_str(&format!(
                            "\n\n--- Attached file: {} ---\n{}\n--- End of {} ---",
                            file_row.filename, content, file_row.filename
                        ));
                    }
                    // Not inlineable (too big / binary / no vision): mention it.
                    _ => {
                        text.push_str(&format!(
                            "\n\n[Attachment: {} ({}, {} bytes)]",
                            file_row.filename, file_row.content_type, file_row.size_bytes
                        ));
                    }
                }
            }
            if !text.trim().is_empty() || !images.is_empty() {
                history.push(LlmMessage::User {
                    content: text,
                    images,
                });
            }
            continue;
        }

        // Assistant: text parts + tool parts → assistant message + tool results.
        let text = message.text();
        let tool_parts = message.tool_parts();
        let mut tool_calls = Vec::new();
        for tp in &tool_parts {
            if tp.input.is_null() {
                continue;
            }
            tool_calls.push(ToolCallReq {
                id: tp.tool_call_id.clone(),
                name: tp.tool_name.clone(),
                arguments: tp.input.clone(),
            });
        }
        history.push(LlmMessage::Assistant {
            content: if text.is_empty() { None } else { Some(text) },
            tool_calls,
        });

        for tp in tool_parts {
            let content = if let Some(output) = &tp.output {
                serde_json::to_string(output).unwrap_or_default()
            } else if let Some(err) = &tp.error_text {
                format!("Tool error: {err}")
            } else {
                match (tp.state.as_str(), tp.approved) {
                    ("approval-responded", Some(true)) => {
                        resume.push(ResumeCall {
                            tool_call_id: tp.tool_call_id.clone(),
                            tool_name: tp.tool_name.clone(),
                            input: tp.input.clone(),
                            stored_message_id: row.id,
                        });
                        continue; // result appended after execution
                    }
                    ("approval-responded", _) => {
                        denied.push((row.id, tp.tool_call_id.clone()));
                        "The user denied this tool call.".to_string()
                    }
                    ("approval-requested", _) => "Awaiting user approval.".to_string(),
                    _ => "Tool call was interrupted before producing output.".to_string(),
                }
            };
            history.push(LlmMessage::ToolResult {
                tool_call_id: tp.tool_call_id.clone(),
                content,
            });
        }
    }

    (history, resume, denied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::MessageRow;
    use crate::service::files::InlinePayload;
    use chrono::Utc;
    use std::collections::HashMap;

    fn no_files() -> HashMap<Uuid, crate::domain::FileRow> {
        HashMap::new()
    }

    fn no_payloads() -> HashMap<Uuid, InlinePayload> {
        HashMap::new()
    }

    /// Change proposals go to a human, so the run must stop after them instead
    /// of letting the model loop into another call.
    #[test]
    fn change_proposals_end_the_turn() {
        use crate::tools::{Tool, meta};

        assert!(meta::ProposeTool.ends_turn());
        assert!(meta::ProposeAgent.ends_turn());
        // Read-only meta tools keep the loop going.
        assert!(!meta::ListTools.ends_turn());
    }

    fn file_row(id: Uuid, filename: &str, content_type: &str, size: i64) -> crate::domain::FileRow {
        crate::domain::FileRow {
            id,
            conversation_id: None,
            message_id: None,
            user_id: Uuid::new_v4(),
            origin: "attachment".into(),
            filename: filename.into(),
            content_type: content_type.into(),
            size_bytes: size,
            storage_key: format!("files/{id}/{filename}"),
            checksum_sha256: None,
            created_at: Utc::now(),
        }
    }

    fn row(role: &str, parts: Value) -> MessageRow {
        MessageRow {
            id: Uuid::new_v4(),
            conversation_id: Uuid::new_v4(),
            run_id: None,
            role: role.into(),
            parts,
            model_id: None,
            usage: None,
            seq: 1,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn history_assembly_basic() {
        let rows = vec![
            row("user", json!([{"type": "text", "text": "read the file"}])),
            row(
                "assistant",
                json!([
                    {"type": "tool-workspace.read", "toolCallId": "c1", "state": "output-available",
                     "input": {"path": "a"}, "output": {"content": "hello"}},
                    {"type": "text", "text": "The file says hello."}
                ]),
            ),
            row("user", json!([{"type": "text", "text": "thanks"}])),
        ];
        let (history, resume, denied) =
            history_from_messages(&rows, &no_files(), &no_payloads(), false);
        assert!(resume.is_empty());
        assert!(denied.is_empty());
        assert_eq!(history.len(), 4);
        assert!(matches!(history[0], LlmMessage::User { .. }));
        match &history[1] {
            LlmMessage::Assistant {
                content,
                tool_calls,
            } => {
                assert_eq!(content.as_deref(), Some("The file says hello."));
                assert_eq!(tool_calls.len(), 1);
            }
            other => panic!("unexpected: {other:?}"),
        }
        match &history[2] {
            LlmMessage::ToolResult {
                tool_call_id,
                content,
            } => {
                assert_eq!(tool_call_id, "c1");
                assert!(content.contains("hello"));
            }
            other => panic!("unexpected: {other:?}"),
        }
        assert!(matches!(history[3], LlmMessage::User { .. }));
    }

    #[test]
    fn history_inline_text_and_note_fallback() {
        let img_id = Uuid::new_v4();
        let csv_id = Uuid::new_v4();
        let zip_id = Uuid::new_v4();
        let mut files = HashMap::new();
        files.insert(img_id, file_row(img_id, "shot.png", "image/png", 4096));
        files.insert(csv_id, file_row(csv_id, "data.csv", "text/csv", 40));
        files.insert(
            zip_id,
            file_row(zip_id, "bundle.zip", "application/zip", 999_999),
        );

        let mut payloads = HashMap::new();
        payloads.insert(
            img_id,
            InlinePayload::ImageUrl("data:image/png;base64,AAAA".to_string()),
        );
        payloads.insert(csv_id, InlinePayload::Text("a,b\n1,2".to_string()));

        let rows = vec![row(
            "user",
            json!([
                {"type": "text", "text": "analyze"},
                {"type": "file", "mediaType": "image/png", "url": format!("/api/files/{img_id}/content")},
                {"type": "file", "mediaType": "text/csv", "filename": "data.csv", "url": format!("/api/files/{csv_id}/content")},
                {"type": "file", "mediaType": "application/zip", "filename": "bundle.zip", "url": format!("/api/files/{zip_id}/content")}
            ]),
        )];

        // Vision model: image becomes a content block, csv inlined, zip noted.
        let (history, _, _) = history_from_messages(&rows, &files, &payloads, true);
        match &history[0] {
            LlmMessage::User { content, images } => {
                assert!(content.contains("--- Attached file: data.csv ---"));
                assert!(content.contains("a,b\n1,2"));
                assert!(
                    content.contains("[Attachment: bundle.zip (application/zip, 999999 bytes)]")
                );
                assert_eq!(images.len(), 1);
                assert!(images[0].starts_with("data:image/png;base64,"));
            }
            other => panic!("unexpected: {other:?}"),
        }

        // Non-vision model: image degrades to a note, csv still inlined.
        let (history, _, _) = history_from_messages(&rows, &files, &payloads, false);
        match &history[0] {
            LlmMessage::User { content, images } => {
                assert!(content.contains("[Attachment: shot.png (image/png, 4096 bytes)]"));
                assert!(content.contains("a,b\n1,2"));
                assert!(images.is_empty());
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn history_assembly_resume_and_denial() {
        let rows = vec![
            row("user", json!([{"type": "text", "text": "clean dist"}])),
            row(
                "assistant",
                json!([
                    {"type": "tool-workspace.write", "toolCallId": "c2", "state": "approval-responded",
                     "input": {"path": "x"}, "approval": {"approved": true}},
                    {"type": "tool-workspace.write", "toolCallId": "c3", "state": "approval-responded",
                     "input": {"path": "y"}, "approval": {"approved": false}}
                ]),
            ),
        ];
        let (history, resume, denied) =
            history_from_messages(&rows, &no_files(), &no_payloads(), false);
        assert_eq!(resume.len(), 1);
        assert_eq!(resume[0].tool_call_id, "c2");
        assert_eq!(denied.len(), 1);
        assert_eq!(denied[0].1, "c3");
        assert_eq!(history.len(), 3); // assistant + denied result (resume result appended later)
        match &history[2] {
            LlmMessage::ToolResult {
                tool_call_id,
                content,
            } => {
                assert_eq!(tool_call_id, "c3");
                assert!(content.contains("denied"));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }
}
