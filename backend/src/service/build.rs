//! Admin-triggered WASM build pipeline.
//!
//! The actual compiler runs in a disposable builder container in production.
//! Locally, `TOOL_BUILDER_CMD` points at a script that accepts
//! `<source-file> <output-wasm>` and writes the artifact to the second path.
//! The build is never registered as an agent tool.

use std::time::Duration;

use bytes::Bytes;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::domain::ToolBuild;
use crate::error::{AppError, AppResult};
use crate::repo;
use crate::state::AppState;

const MAX_ARTIFACT_BYTES: usize = 32 * 1024 * 1024;
const BUILD_TIMEOUT: Duration = Duration::from_secs(180);

pub async fn start(state: &AppState, tool_id: Uuid, admin_id: Uuid) -> AppResult<ToolBuild> {
    let tool = repo::tools::by_id(&state.pool, tool_id)
        .await?
        .ok_or_else(|| AppError::not_found("tool not found"))?;
    if tool.kind != "wasm" {
        return Err(AppError::validation("only wasm tools can be built"));
    }

    let build = repo::tool_builds::latest(&state.pool, tool_id)
        .await?
        .filter(|b| b.status == "pending" || b.status == "failed")
        .ok_or_else(|| AppError::conflict("no pending build for this tool"))?;

    repo::tool_builds::mark_building(&state.pool, build.id).await?;

    let Some(cmd) = std::env::var("TOOL_BUILDER_CMD")
        .ok()
        .filter(|s| !s.trim().is_empty())
    else {
        repo::tool_builds::mark_failed(
            &state.pool,
            build.id,
            "TOOL_BUILDER_CMD is not configured; run the builder container or set the command",
        )
        .await?;
        return Err(AppError::bad_request(
            "tool builder is not configured (set TOOL_BUILDER_CMD)",
        ));
    };

    let dir = std::env::temp_dir().join(format!("assistant-build-{}", Uuid::new_v4()));
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| AppError::internal(format!("cannot create build dir: {e}")))?;
    let source_path = dir.join("source.txt");
    let out_path = dir.join("out.wasm");
    tokio::fs::write(&source_path, build.source.as_bytes())
        .await
        .map_err(|e| AppError::internal(format!("cannot write source: {e}")))?;

    let output = match tokio::time::timeout(
        BUILD_TIMEOUT,
        tokio::process::Command::new(&cmd)
            .arg(&source_path)
            .arg(&out_path)
            .current_dir(&dir)
            .output(),
    )
    .await
    {
        Err(_) => {
            let log = "build timed out".to_string();
            repo::tool_builds::mark_failed(&state.pool, build.id, &log).await?;
            cleanup(&dir).await;
            return Err(AppError::bad_request("tool build timed out"));
        }
        Ok(Err(e)) => {
            let log = format!("cannot run builder: {e}");
            repo::tool_builds::mark_failed(&state.pool, build.id, &log).await?;
            cleanup(&dir).await;
            return Err(AppError::internal(log));
        }
        Ok(Ok(output)) => output,
    };

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let log = format!(
        "exit: {:?}\n--- stdout ---\n{}\n--- stderr ---\n{}",
        output.status.code(),
        stdout,
        stderr
    );

    if !output.status.success() {
        repo::tool_builds::mark_failed(&state.pool, build.id, &log).await?;
        cleanup(&dir).await;
        return Err(AppError::bad_request(format!(
            "tool build failed (exit {:?})",
            output.status.code()
        )));
    }

    let bytes = tokio::fs::read(&out_path)
        .await
        .map_err(|e| AppError::internal(format!("cannot read artifact: {e}")))?;
    if bytes.is_empty() {
        repo::tool_builds::mark_failed(&state.pool, build.id, "builder produced no artifact")
            .await?;
        cleanup(&dir).await;
        return Err(AppError::bad_request("builder produced no artifact"));
    }
    if bytes.len() > MAX_ARTIFACT_BYTES {
        repo::tool_builds::mark_failed(&state.pool, build.id, "artifact is too large").await?;
        cleanup(&dir).await;
        return Err(AppError::payload_too_large("artifact is too large"));
    }

    let sha256 = hex::encode(Sha256::digest(&bytes));
    let storage_key = format!("tool-builds/{tool_id}/{}.wasm", build.revision);
    state
        .storage
        .put(&storage_key, Bytes::from(bytes.clone()))
        .await
        .map_err(|e| AppError::internal(format!("cannot store artifact: {e}")))?;

    repo::tool_builds::mark_built(
        &state.pool,
        build.id,
        &storage_key,
        &sha256,
        bytes.len() as i64,
        &log,
    )
    .await?;
    sqlx::query("UPDATE tools SET status = 'enabled', updated_at = now() WHERE id = $1")
        .bind(tool_id)
        .execute(&state.pool)
        .await?;
    repo::audit::insert(
        &state.pool,
        Some(admin_id),
        "tool.build",
        &format!("tool={tool_id} revision={}", build.revision),
        true,
        serde_json::json!({"sha256": sha256, "size_bytes": bytes.len()}),
    )
    .await
    .ok();

    cleanup(&dir).await;
    repo::tool_builds::by_tool_revision(&state.pool, tool_id, build.revision)
        .await?
        .ok_or_else(|| AppError::internal("build row disappeared"))
}

async fn cleanup(dir: &std::path::Path) {
    let _ = tokio::fs::remove_dir_all(dir).await;
}
