//! `switch_agent` — hands the turn to another agent.
//!
//! The tool itself does not run anything: [`Tool::switch_target`] tells the
//! engine which agent to hand control to, and the engine swaps its active agent
//! before the next model call. That keeps streaming, tool approval and usage
//! accounting identical to a normal run.

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::error::{AppError, AppResult};
use crate::repo;
use crate::tools::{Tool, ToolDefinition, ToolExecContext};

pub struct SwitchAgent;

#[async_trait]
impl Tool for SwitchAgent {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "switch_agent".into(),
            description: "Hand the conversation to a specialist agent when it fits the task \
                          better than you do — for example `tool-writer` to design a new tool. \
                          The specialist takes over with its own prompt, model and tools, \
                          answers the task, and then the turn ends with its answer."
                .into(),
            parameters_schema: json!({
                "type": "object",
                "properties": {
                    "agent": {
                        "type": "string",
                        "description": "Slug of the agent to switch to"
                    },
                    "task": {
                        "type": "string",
                        "description": "Complete, self-contained instructions for the specialist"
                    }
                },
                "required": ["agent", "task"]
            }),
        }
    }

    fn switch_target(&self, input: &Value) -> Option<String> {
        input
            .get("agent")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    }

    async fn execute(&self, ctx: &ToolExecContext, input: Value) -> AppResult<Value> {
        // The engine intercepts this call, so reaching here means the switch
        // could not be performed. Re-validate to produce a useful error.
        if ctx.switch_depth >= 1 {
            return Err(AppError::bad_request(
                "agent switch depth exceeded — a specialist cannot switch again",
            ));
        }
        let slug = self
            .switch_target(&input)
            .ok_or_else(|| AppError::validation("agent is required"))?;
        if slug == ctx.agent_slug {
            return Err(AppError::validation("already running that agent"));
        }
        let target = repo::agents::by_slug(&ctx.pool, &slug)
            .await?
            .filter(|a| a.status == "enabled")
            .ok_or_else(|| AppError::not_found(format!("agent '{slug}' not found")))?;
        let task = input
            .get("task")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        Ok(json!({
            "switch_to": target.slug,
            "name": target.name,
            "task": task,
        }))
    }
}
