use std::sync::Arc;

use axum::Router;
use axum::http::{StatusCode, header::HeaderName};
use axum::routing::{delete, get, patch, post, put};
use tower_http::cors::CorsLayer;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};

use crate::state::AppState;

const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

pub fn build_router(state: Arc<AppState>) -> Router {
    let api = Router::new()
        // health
        .route("/health", get(health))
        // auth
        .route("/meta", get(crate::api::auth::meta))
        .route("/auth/register", post(crate::api::auth::register))
        .route("/auth/login", post(crate::api::auth::login))
        .route(
            "/auth/me",
            get(crate::api::auth::me).put(crate::api::auth::update_profile),
        )
        // preferences
        .route(
            "/me/preferences",
            get(crate::api::preferences::get).put(crate::api::preferences::update),
        )
        // chat
        .route("/chat", post(crate::api::chat::chat))
        .route("/chat/cancel", post(crate::api::chat::cancel))
        .route("/chat/resume", get(crate::api::chat::resume))
        // models (Model Selector — any signed-in user)
        .route("/models", get(crate::api::models::list))
        // agents (Agent Selector — any signed-in user, chat-kind only)
        .route("/agents", get(crate::api::agents::list))
        // conversations
        .route("/conversations", get(crate::api::conversations::list))
        .route(
            "/conversations/{id}",
            delete(crate::api::conversations::delete).put(crate::api::conversations::rename),
        )
        .route(
            "/conversations/{id}/messages",
            get(crate::api::conversations::messages),
        )
        .route(
            "/conversations/{id}/title",
            post(crate::api::conversations::generate_title),
        )
        .route(
            "/conversations/{id}/group",
            put(crate::api::conversations::set_group),
        )
        // conversation folders (one level, per user)
        .route(
            "/conversation-groups",
            get(crate::api::conversation_groups::list)
                .post(crate::api::conversation_groups::create),
        )
        .route(
            "/conversation-groups/{id}",
            put(crate::api::conversation_groups::rename)
                .delete(crate::api::conversation_groups::remove),
        )
        // workspace exploration (agent sandbox files)
        .route(
            "/conversations/{id}/workspace",
            get(crate::api::workspace::tree),
        )
        .route(
            "/conversations/{id}/workspace/content",
            get(crate::api::workspace::content),
        )
        // files (attachments + artifacts). The upload route raises axum's
        // default 2MB body limit to the configured cap (+ multipart framing).
        .route(
            "/files",
            post(crate::api::files::upload).layer(axum::extract::DefaultBodyLimit::max(
                state.cfg.max_upload_bytes + 1024 * 1024,
            )),
        )
        .route("/files/{id}", delete(crate::api::files::remove))
        .route("/files/{id}/content", get(crate::api::files::download))
        // feedback on tool/agent runs
        .route(
            "/feedback",
            post(crate::api::feedback::submit).get(crate::api::feedback::list),
        )
        .route("/feedback/{id}", delete(crate::api::feedback::delete))
        // approvals
        .route("/approvals/{id}", post(crate::api::approvals::decide))
        .route(
            "/conversations/{id}/approvals",
            get(crate::api::approvals::list_for_conversation),
        )
        // admin — providers/models
        .route(
            "/admin/provider-presets",
            get(crate::api::admin::providers::presets),
        )
        .route(
            "/admin/providers",
            get(crate::api::admin::providers::list).post(crate::api::admin::providers::create),
        )
        .route(
            "/admin/providers/{id}",
            put(crate::api::admin::providers::update).delete(crate::api::admin::providers::delete),
        )
        .route(
            "/admin/providers/{id}/models",
            get(crate::api::admin::providers::list_models)
                .post(crate::api::admin::providers::create_model),
        )
        .route(
            "/admin/providers/{id}/models/discover",
            get(crate::api::admin::providers::discover_models),
        )
        .route(
            "/admin/providers/{id}/models/import",
            post(crate::api::admin::providers::import_models),
        )
        .route(
            "/admin/models/{id}",
            put(crate::api::admin::providers::update_model)
                .delete(crate::api::admin::providers::delete_model),
        )
        // admin — agents
        .route(
            "/admin/agents",
            get(crate::api::admin::agents::list).post(crate::api::admin::agents::create),
        )
        .route(
            "/admin/agents/{id}",
            put(crate::api::admin::agents::update).delete(crate::api::admin::agents::delete),
        )
        .route(
            "/admin/agents/{id}/tools",
            get(crate::api::admin::agents::list_tools).post(crate::api::admin::agents::assign_tool),
        )
        .route(
            "/admin/agents/{id}/tools/{tool_id}",
            delete(crate::api::admin::agents::unassign_tool),
        )
        .route(
            "/admin/agents/{id}/versions",
            get(crate::api::admin::versions::list_agent_versions),
        )
        .route(
            "/admin/agents/{id}/versions/{revision}/restore",
            post(crate::api::admin::versions::restore_agent),
        )
        // admin — tool + agent folders
        .route(
            "/admin/tool-groups",
            get(crate::api::admin::tool_groups::list).post(crate::api::admin::tool_groups::create),
        )
        .route(
            "/admin/tool-groups/{id}",
            put(crate::api::admin::tool_groups::update)
                .delete(crate::api::admin::tool_groups::remove),
        )
        .route(
            "/admin/agent-groups",
            get(crate::api::admin::agent_groups::list)
                .post(crate::api::admin::agent_groups::create),
        )
        .route(
            "/admin/agent-groups/{id}",
            put(crate::api::admin::agent_groups::update)
                .delete(crate::api::admin::agent_groups::remove),
        )
        // admin — tools + builds
        .route(
            "/admin/tools",
            get(crate::api::admin::tools::list).post(crate::api::admin::tools::create),
        )
        .route(
            "/admin/tools/{id}",
            put(crate::api::admin::tools::update).delete(crate::api::admin::tools::delete),
        )
        .route(
            "/admin/tools/{id}/build",
            post(crate::api::admin::tool_builds::start),
        )
        .route(
            "/admin/tools/{id}/test",
            post(crate::api::admin::tool_builds::test),
        )
        .route(
            "/admin/tools/{id}/builds",
            get(crate::api::admin::tool_builds::list),
        )
        .route(
            "/admin/tools/{id}/builds/{revision}",
            get(crate::api::admin::tool_builds::get),
        )
        .route(
            "/admin/tools/{id}/versions",
            get(crate::api::admin::versions::list_tool_versions),
        )
        .route(
            "/admin/tools/{id}/versions/{revision}/restore",
            post(crate::api::admin::versions::restore_tool),
        )
        // admin — proposals + feedback
        .route("/admin/proposals", get(crate::api::admin::proposals::list))
        .route(
            "/admin/proposals/{id}",
            get(crate::api::admin::proposals::get),
        )
        .route(
            "/admin/proposals/{id}/apply",
            post(crate::api::admin::proposals::apply),
        )
        .route(
            "/admin/proposals/{id}/reject",
            post(crate::api::admin::proposals::reject),
        )
        .route("/admin/feedback", get(crate::api::admin::feedback::list))
        .route(
            "/admin/feedback/{id}",
            patch(crate::api::admin::feedback::update),
        )
        // admin — secrets
        .route(
            "/admin/secrets",
            get(crate::api::admin::secrets::list).post(crate::api::admin::secrets::create),
        )
        .route(
            "/admin/secrets/{id}",
            put(crate::api::admin::secrets::update).delete(crate::api::admin::secrets::delete),
        )
        .with_state(Arc::clone(&state));

    let mut app = Router::new().nest("/api", api);

    // Static SPA (production): serve dist with index.html fallback.
    if let Some(static_dir) = state.cfg.static_dir.clone() {
        let index = static_dir.join("index.html");
        let serve = tower_http::services::ServeDir::new(&static_dir)
            .fallback(tower_http::services::ServeFile::new(index));
        app = app.fallback_service(serve);
    }

    app.layer(PropagateRequestIdLayer::new(REQUEST_ID_HEADER.clone()))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().include_headers(true))
                .on_response(DefaultOnResponse::new()),
        )
        .layer(SetRequestIdLayer::new(
            REQUEST_ID_HEADER.clone(),
            MakeRequestUuid,
        ))
        .layer(tower_http::catch_panic::CatchPanicLayer::new())
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn health() -> StatusCode {
    StatusCode::OK
}
