//! Chat orchestration: request → run guard → agent execution → SSE stream.

use std::sync::Arc;

use axum::response::sse::{Event, KeepAlive, Sse};
use futures::stream;
use serde::Deserialize;
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::agent::{self, PreparedRun, RunCollector};
use crate::aistream::Chunk;
use crate::api::AuthUser;
use crate::domain::ui_message::UIMessage;
use crate::domain::{Agent, Model};
use crate::error::{AppError, AppResult};
use crate::llm::ProviderCredentials;
use crate::repo;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub conversation_id: Option<Uuid>,
    pub agent_slug: Option<String>,
    pub model_id: Option<Uuid>,
    /// Per-request reasoning effort override (`low|medium|high`); absent →
    /// the model's configured default.
    pub reasoning_effort: Option<String>,
    /// Folder for a conversation that is created by this request. Ignored
    /// once the conversation already exists.
    pub group_id: Option<Uuid>,
    pub messages: Vec<UIMessage>,
}

/// Only allow assigning a folder the caller owns.
async fn validate_group(state: &AppState, group_id: Option<Uuid>, user_id: Uuid) -> AppResult<()> {
    if let Some(group_id) = group_id {
        repo::conversation_groups::owned_by_id(&state.pool, group_id, user_id).await?;
    }
    Ok(())
}

/// Resolve the agent for a chat request.
///
/// Phase 1: the chat surface always runs on `general`, which itself decides
/// which agent (and tools) should carry out the task. Any other slug is
/// ignored here, so a client cannot bind a conversation to another agent.
/// Studio's `meta` agents stay reachable for admins only.
async fn resolve_agent(state: &AppState, slug: Option<&str>, is_admin: bool) -> AppResult<Agent> {
    // Non-admins are pinned to `general` regardless of what they send.
    let slug = if is_admin { slug } else { None };

    let found = if let Some(slug) = slug {
        repo::agents::by_slug(&state.pool, slug)
            .await?
            .filter(|a| a.status == "enabled")
            .ok_or_else(|| AppError::not_found(format!("agent '{slug}' not found")))?
    } else if let Some(a) = repo::agents::by_slug(&state.pool, "general")
        .await?
        .filter(|a| a.status == "enabled")
    {
        a
    } else {
        repo::agents::list(&state.pool)
            .await?
            .into_iter()
            .find(|a| a.status == "enabled" && (is_admin || a.kind == "chat"))
            .ok_or_else(|| {
                AppError::bad_request("no agent configured — create one in Admin → Agents")
            })?
    };
    if found.kind != "chat" && !is_admin {
        return Err(AppError::forbidden(
            "this agent is not available in chat — ask an admin",
        ));
    }
    Ok(found)
}

/// Model resolution chain: request override → conversation → agent default → global default.
async fn resolve_model(
    state: &AppState,
    requested: Option<Uuid>,
    conversation_model: Option<Uuid>,
    agent: &Agent,
) -> AppResult<Model> {
    resolve_model_pool(&state.pool, requested, conversation_model, agent).await
}

/// `resolve_model` without `AppState` — reusable by the agent-switch path.
pub(crate) async fn resolve_model_pool(
    pool: &sqlx::PgPool,
    requested: Option<Uuid>,
    conversation_model: Option<Uuid>,
    agent: &Agent,
) -> AppResult<Model> {
    let candidates = [requested, conversation_model, agent.default_model_id];
    for candidate in candidates.into_iter().flatten() {
        if let Some(m) = repo::providers::model_by_id(pool, candidate).await? {
            if m.status == "enabled" {
                return Ok(m);
            }
        }
    }
    repo::providers::default_model(pool)
        .await?
        .ok_or_else(|| {
            AppError::bad_request("no model configured — add a provider and model in Admin")
        })
}

pub(crate) async fn provider_credentials(
    state: &AppState,
    model: &Model,
) -> AppResult<(Arc<dyn crate::llm::Provider>, ProviderCredentials)> {
    provider_credentials_with(
        &state.pool,
        &state.secrets,
        &state.providers,
        model,
    )
    .await
}

/// `provider_credentials` without `AppState` — reusable by the agent-switch path.
pub(crate) async fn provider_credentials_with(
    pool: &sqlx::PgPool,
    secrets: &crate::secrets::SecretResolver,
    providers: &std::collections::HashMap<String, Arc<dyn crate::llm::Provider>>,
    model: &Model,
) -> AppResult<(Arc<dyn crate::llm::Provider>, ProviderCredentials)> {
    let provider_row = repo::providers::provider_by_id(pool, model.provider_id)
        .await?
        .ok_or_else(|| AppError::internal("model references missing provider"))?;
    if provider_row.status != "enabled" {
        return Err(AppError::bad_request("provider is disabled"));
    }
    // All presets speak the OpenAI Chat Completions wire format today.
    let provider = providers
        .get("openaicompat")
        .cloned()
        .ok_or_else(|| AppError::internal("openaicompat provider missing from registry"))?;
    let api_key = match provider_row.secret_id {
        Some(secret_id) => {
            // Look the secret up by id → name → resolver.
            let name = sqlx::query_scalar::<_, String>("SELECT name FROM secrets WHERE id = $1")
                .bind(secret_id)
                .fetch_optional(pool)
                .await?
                .ok_or_else(|| {
                    AppError::bad_request("provider secret was deleted — reconfigure the provider")
                })?;
            Some(secrets.get(&name).await?)
        }
        None => None,
    };
    Ok((
        provider,
        ProviderCredentials {
            base_url: provider_row.base_url,
            api_key,
        },
    ))
}

/// Reconcile client-asserted approval responses against the server-side ledger.
/// The ledger is the source of truth; client assertions that disagree are
/// overridden and audited.
async fn reconcile_approvals(
    state: &AppState,
    user_id: Uuid,
    conversation_id: Uuid,
    messages: &[UIMessage],
) -> AppResult<()> {
    for message in messages.iter().filter(|m| !m.is_user()) {
        for tp in message.tool_parts() {
            if tp.state != "approval-responded" {
                continue;
            }
            let Some(stored) = repo::conversations::by_conversation_and_tool_call(
                &state.pool,
                conversation_id,
                &tp.tool_call_id,
            )
            .await?
            else {
                continue;
            };
            let ledger = repo::approvals::by_conversation_and_call(
                &state.pool,
                conversation_id,
                &tp.tool_call_id,
            )
            .await?
            .ok_or_else(|| AppError::bad_request("approval record missing"))?;

            // The ledger wins.
            let (state_str, approved_flag): (&str, bool) = match ledger.status.as_str() {
                "approved" => ("approval-responded", true),
                "denied" => ("approval-responded", false),
                _ => ("approval-requested", false),
            };
            if tp.approved != Some(approved_flag) {
                crate::repo::audit::insert(
                    &state.pool,
                    Some(user_id),
                    "approval.mismatch",
                    &format!("tool_call={}", tp.tool_call_id),
                    false,
                    serde_json::json!({"client_claimed": tp.approved, "ledger": ledger.status}),
                )
                .await
                .ok();
            }
            let parts = stored.parts.clone();
            if let Some(arr) = parts.as_array() {
                let mut new_parts = parts.clone();
                if let Some(arr_mut) = new_parts.as_array_mut() {
                    for (i, part) in arr.iter().enumerate() {
                        let is_target = part.get("toolCallId").and_then(Value::as_str)
                            == Some(tp.tool_call_id.as_str())
                            && part
                                .get("type")
                                .and_then(Value::as_str)
                                .is_some_and(|t| t.starts_with("tool-"));
                        if is_target {
                            if let Some(p) = arr_mut.get_mut(i) {
                                p["state"] = serde_json::json!(state_str);
                                p["approval"] = serde_json::json!({"approved": approved_flag});
                            }
                        }
                    }
                }
                repo::conversations::upsert_message_parts(
                    &state.pool,
                    stored.id,
                    new_parts,
                    None,
                    None,
                )
                .await?;
            }
        }
    }
    Ok(())
}

pub async fn run_chat(
    state: Arc<AppState>,
    user: AuthUser,
    req: ChatRequest,
) -> AppResult<Sse<impl futures::Stream<Item = Result<Event, std::convert::Infallible>>>> {
    if req.messages.is_empty() {
        return Err(AppError::bad_request("messages must not be empty"));
    }
    let Some(user_message) = req.messages.iter().rev().find(|m| m.is_user()) else {
        return Err(AppError::bad_request(
            "the last message must be from the user",
        ));
    };
    let is_admin = user.role == "admin";

    // ----- Conversation (create or verify) -----
    // The client pre-allocates conversation ids (uuid v4) so its Chat instance
    // is stable from the first message; the server materializes the row on
    // first use. Existing ids are ownership-checked as usual.
    let title: String = {
        let first_filename = user_message
            .file_parts()
            .first()
            .and_then(|fp| fp.filename.clone());
        let base = user_message.text();
        let (source_text, is_file) = if base.trim().is_empty() {
            match first_filename {
                Some(name) => (name, true),
                None => (String::new(), false),
            }
        } else {
            (base, false)
        };
        let t = source_text.trim();
        let mut s: String = t.chars().take(60).collect();
        if t.chars().count() > 60 {
            s.push('…');
        }
        if is_file {
            s = format!("📎 {s}");
        }
        if s.is_empty() {
            "New conversation".to_string()
        } else {
            s
        }
    };
    let conversation = match req.conversation_id {
        Some(id) => match repo::conversations::by_id(&state.pool, id).await? {
            Some(conv) => {
                if conv.user_id != user.id {
                    return Err(AppError::not_found("conversation not found"));
                }
                if conv.status == "running" || state.runs.contains_key(&conv.id) {
                    return Err(AppError::conflict(
                        "a run is already active for this conversation",
                    ));
                }
                conv
            }
            None => {
                validate_group(&state, req.group_id, user.id).await?;
                let agent = resolve_agent(&state, req.agent_slug.as_deref(), is_admin).await?;
                repo::conversations::create_with_id(
                    &state.pool,
                    id,
                    user.id,
                    agent.id,
                    &title,
                    None,
                    req.group_id,
                )
                .await?
            }
        },
        None => {
            validate_group(&state, req.group_id, user.id).await?;
            let agent = resolve_agent(&state, req.agent_slug.as_deref(), is_admin).await?;
            repo::conversations::create(&state.pool, user.id, agent.id, &title, None, req.group_id)
                .await?
        }
    };
    let agent = if req.agent_slug.is_some() && req.conversation_id.is_none() {
        resolve_agent(&state, req.agent_slug.as_deref(), is_admin).await?
    } else {
        repo::agents::by_id(&state.pool, conversation.agent_id)
            .await?
            .ok_or_else(|| AppError::internal("conversation references missing agent"))?
    };

    // ----- Model + provider credentials -----
    let model = resolve_model(&state, req.model_id, conversation.model_id, &agent).await?;
    let (provider, credentials) = provider_credentials(&state, &model).await?;

    // Reasoning effort: request override → model default → none.
    let reasoning_effort = match req.reasoning_effort.as_deref() {
        Some(raw) => crate::domain::normalize_effort(raw).map_err(AppError::validation)?,
        None => None,
    }
    .or(model.reasoning_effort.clone());

    // ----- Run guard -----
    if state.runs.contains_key(&conversation.id) {
        return Err(AppError::conflict(
            "a run is already active for this conversation",
        ));
    }
    let token = CancellationToken::new();
    state.runs.insert(conversation.id, token.clone());
    repo::conversations::set_status(&state.pool, conversation.id, "running").await?;

    // ----- Run ledger (stable identity for feedback) -----
    let run = repo::runs::create(&state.pool, conversation.id, agent.id, Some(model.id)).await?;

    // ----- Persist the user message -----
    // Only a genuinely trailing user turn opens a new row. Approval-resume
    // sends re-post the whole transcript with the assistant approval response
    // last — appending the user message again would read to the model as the
    // user asking twice, which makes it re-run the tool.
    let user_turn_is_new = req.messages.last().is_some_and(|m| m.is_user());
    if user_turn_is_new {
        let user_parts = serde_json::to_value(&user_message.parts)?;
        let saved = repo::conversations::append_message(
            &state.pool,
            conversation.id,
            None,
            "user",
            user_parts,
            None,
        )
        .await?;
        // Link referenced attachments (ownership-checked) and copy them into
        // the sandbox so workspace/file tools can work with them during the run.
        crate::service::files::activate_attachments(
            &state,
            user.id,
            conversation.id,
            saved.id,
            &user_message.parts,
            &agent::sandbox_root_for(&state.cfg, conversation.id),
        )
        .await?;
    }

    // ----- Reconcile approval responses (ledger wins) -----
    reconcile_approvals(&state, user.id, conversation.id, &req.messages).await?;

    // ----- History + resume calls from stored parts -----
    let stored = repo::conversations::messages(&state.pool, conversation.id).await?;
    // Attachment metadata + inline payloads feed the multimodal history.
    let conversation_files = repo::files::by_conversation(&state.pool, conversation.id).await?;
    let vision = model.supports_vision();
    let payloads =
        crate::service::files::resolve_inline_payloads(&state.storage, &conversation_files, vision)
            .await;
    let files_by_id: std::collections::HashMap<uuid::Uuid, crate::domain::FileRow> =
        conversation_files.into_iter().map(|f| (f.id, f)).collect();
    let (history, resume_calls, denied_calls) =
        agent::history_from_messages(&stored, &files_by_id, &payloads, vision);

    // ----- Tool registry + execution context -----
    let registry = agent::build_registry(
        &state.pool,
        agent.id,
        state.cfg.searxng_url.as_deref(),
        30_000,
        64 * 1024,
    )
    .await?;
    let assignments = repo::agents::tools_for_agent(&state.pool, agent.id).await?;
    let mut ctx = crate::tools::ToolExecContext {
        user_id: user.id,
        agent_id: agent.id,
        conversation_id: conversation.id,
        run_id: run.id,
        cancel: token.clone(),
        mode: state.cfg.mode,
        sandbox_root: agent::sandbox_root_for(&state.cfg, conversation.id),
        allowed_paths: agent::allowed_paths_from(&assignments),
        storage: Arc::clone(&state.storage),
        max_file_bytes: state.cfg.max_upload_bytes,
        secrets: Arc::clone(&state.secrets),
        http: state.http.clone(),
        pool: state.pool.clone(),
        wasm: Arc::clone(&state.wasm),
        agent_slug: agent.slug.clone(),
        switch_depth: 0,
        providers: state.providers.clone(),
        searxng_url: state.cfg.searxng_url.clone(),
        default_tool_timeout_ms: 30_000,
        default_tool_output_cap: 64 * 1024,
    };

    let prepared = PreparedRun {
        run_id: run.id,
        reasoning_effort,
    };

    // The agent driving this turn. A `switch_agent` call replaces `active`
    // mid-run; `home` is the conversation's own agent.
    let mut agents = agent::RunAgents {
        active: agent.clone(),
        home: agent.clone(),
        model: model.clone(),
        provider: provider.clone(),
        credentials: credentials.clone(),
        registry,
        switch_depth: 0,
    };

    // A previous request may have ended mid-switch: resume under the agent the
    // transcript says is active, so its tools (and pending approval) apply.
    if let Some(active_slug) = stored
        .last()
        .and_then(|m| agent::active_agent_slug(m.parts.as_array()?))
        .filter(|slug| *slug != agent.slug)
    {
        let resumed = agent::resume_switched_agent(&state, &mut ctx, &active_slug, &mut agents)
            .await?;
        if !resumed {
            tracing::warn!(slug = %active_slug, "switch target no longer available");
        }
    }

    // ----- Spawn the run; stream chunks over an unbounded channel -----
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Chunk>();
    let conv_id = conversation.id;
    let run_id = run.id;
    let model_id = model.id;
    let pool = state.pool.clone();
    let runs = Arc::clone(&state.runs);
    let user_id = user.id;
    let state_for_title = Arc::clone(&state);

    // Replayable stream so a reloaded tab can reconnect and follow the run.
    let run_stream = Arc::new(crate::service::run_stream::RunStream::new());
    state.run_streams.insert(conv_id, Arc::clone(&run_stream));

    // Persist partial assistant parts while the run is going so clients that
    // poll the transcript (reload / another tab) can follow progress. The
    // first non-empty snapshot creates the assistant message; later ones just
    // replace its parts.
    let (snapshot_tx, mut snapshot_rx) = tokio::sync::watch::channel::<Vec<Value>>(Vec::new());
    let snapshot_pool = state.pool.clone();
    let snapshot_task = tokio::spawn(async move {
        let mut message_id: Option<Uuid> = None;
        while snapshot_rx.changed().await.is_ok() {
            let parts = snapshot_rx.borrow_and_update().clone();
            if parts.is_empty() {
                continue;
            }
            match message_id {
                Some(id) => {
                    let _ = repo::conversations::upsert_message_parts(
                        &snapshot_pool,
                        id,
                        Value::Array(parts),
                        None,
                        None,
                    )
                    .await;
                }
                None => {
                    if let Ok(saved) = repo::conversations::append_message(
                        &snapshot_pool,
                        conv_id,
                        Some(run_id),
                        "assistant",
                        Value::Array(parts),
                        Some(model_id),
                    )
                    .await
                    {
                        message_id = Some(saved.id);
                    }
                }
            }
        }
        message_id
    });

    let agent_slug = agents.home.slug.clone();
    tokio::spawn(async move {
        let mut collector = RunCollector::new(tx)
            .with_snapshot(snapshot_tx)
            .with_stream(Arc::clone(&run_stream));
        // Non-persisted meta part so the client learns the conversation id.
        collector.emit(Chunk::Data {
            name: "conversation".into(),
            id: "meta".into(),
            data: serde_json::json!({
                "conversationId": conv_id,
                "agentSlug": agent_slug,
                "modelId": model_id,
            }),
        });
        let status = agent::execute_run(
            &prepared,
            &mut agents,
            &mut ctx,
            agent::RunInput {
                history,
                resume_calls,
                denied_calls,
            },
            &mut collector,
            &token,
        )
        .await;

        // Final snapshot, then wait for the writer so the stored message is
        // complete before the run is marked finished.
        collector.flush_snapshot();
        drop(collector);
        let assistant_message_id = snapshot_task.await.unwrap_or(None);

        if assistant_message_id.is_some() {
            if let (Some(p), Some(c)) = (status.usage_prompt, status.usage_completion) {
                let _ = sqlx::query("INSERT INTO usage_logs (user_id, conversation_id, model_id, prompt_tokens, completion_tokens) VALUES ($1, $2, $3, $4, $5)")
                    .bind(user_id)
                    .bind(conv_id)
                    .bind(model_id)
                    .bind(p as i32)
                    .bind(c as i32)
                    .execute(&pool)
                    .await;
            }
        }

        let run_status = if status.cancelled {
            "cancelled"
        } else if status.approval_pending {
            "approval_pending"
        } else {
            "completed"
        };
        repo::runs::finish(&pool, run_id, run_status, assistant_message_id)
            .await
            .ok();

        // Cleanup: always release the run.
        repo::conversations::set_status(&pool, conv_id, "idle")
            .await
            .ok();
        runs.remove(&conv_id);
        // Close live resume subscribers; the buffer stays replayable.
        run_stream.finish();

        // First completed turn: upgrade the heuristic title with an LLM one.
        // Detached and best-effort — must not delay the run or surface errors.
        if run_status == "completed" {
            let state_for_title = Arc::clone(&state_for_title);
            tokio::spawn(async move {
                if let Err(e) = crate::service::title::generate_for_conversation(
                    &state_for_title,
                    conv_id,
                    user_id,
                )
                .await
                {
                    tracing::warn!(
                        error = %e,
                        conversation_id = %conv_id,
                        "conversation title generation failed"
                    );
                }
            });
        }
    });

    let stream = stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|chunk| {
            let event = Event::default().data(chunk.to_wire());
            (Ok(event), rx)
        })
    });

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// Handle a cancel request for the caller's active conversation run.
pub async fn cancel_chat(
    state: Arc<AppState>,
    user: AuthUser,
    conversation_id: Uuid,
) -> AppResult<()> {
    let _conv = repo::conversations::owned_by_id(&state.pool, conversation_id, user.id).await?;
    if let Some(entry) = state.runs.get(&conversation_id) {
        entry.value().cancel();
    }
    Ok(())
}
