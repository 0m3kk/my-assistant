//! Capability host functions for `wasm` tools.
//!
//! The guest imports from the `"host"` module:
//!   http(method, url, headers, body) -> result
//!   fs_read(path) -> result
//!   fs_write(path, content) -> result
//!   fs_list(path) -> result
//!   secret_get(name) -> result
//!   log(message)
//!
//! Every `-> result` is `(ptr << 32) | len`, pointing to a JSON string written
//! into the guest's memory via its exported `alloc`.

use std::time::Duration;

use serde_json::{Value, json};
use wasmtime::{Caller, Engine, Linker, TypedFunc};
use wasmtime_wasi::preview1::{self, WasiP1Ctx};

use crate::error::{AppError, AppResult};
use crate::tools::Tool;
use crate::tools::ToolExecContext;
use crate::tools::file::FileTool;

pub struct HostCtx {
    pub wasi: WasiP1Ctx,
    pub env: Option<ToolExecContext>,
    pub caps: Value,
    pub limits: wasmtime::StoreLimits,
    pub alloc: Option<TypedFunc<i32, i32>>,
    pub handle: tokio::runtime::Handle,
    pub logs: Vec<String>,
}

pub fn build_linker(engine: &Engine) -> AppResult<Linker<HostCtx>> {
    let mut linker = Linker::new(engine);
    preview1::add_to_linker_sync(&mut linker, |t: &mut HostCtx| &mut t.wasi)
        .map_err(|e| AppError::internal(format!("wasi linker: {e}")))?;

    linker
        .func_wrap(
            "host",
            "http",
            |mut caller: Caller<'_, HostCtx>,
             method_ptr: i32,
             method_len: i32,
             url_ptr: i32,
             url_len: i32,
             headers_ptr: i32,
             headers_len: i32,
             body_ptr: i32,
             body_len: i32|
             -> i64 {
                let method = read_string(&mut caller, method_ptr, method_len).unwrap_or_default();
                let url = read_string(&mut caller, url_ptr, url_len).unwrap_or_default();
                let headers =
                    read_string(&mut caller, headers_ptr, headers_len).unwrap_or_default();
                let body = read_string(&mut caller, body_ptr, body_len).unwrap_or_default();
                let caps = caller.data().caps.clone();
                if !host_allowed(&caps, &url) {
                    return write_json(&mut caller, json!({"error": "host not allowed"}));
                }
                let Some(env) = caller.data().env.clone() else {
                    return write_json(&mut caller, json!({"error": "no execution context"}));
                };
                let handle = caller.data().handle.clone();
                let result = handle.block_on(async move {
                    let client = reqwest::Client::builder()
                        .redirect(reqwest::redirect::Policy::none())
                        .timeout(Duration::from_secs(30))
                        .build()
                        .map_err(|e| e.to_string())?;
                    let method = reqwest::Method::from_bytes(method.as_bytes())
                        .map_err(|e| e.to_string())?;
                    let mut req = client.request(method, &url);
                    if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&headers) {
                        for (k, v) in map {
                            if let Some(v) = v.as_str() {
                                req = req.header(k, v);
                            }
                        }
                    }
                    if !body.is_empty() {
                        req = req.body(body);
                    }
                    let resp = req.send().await.map_err(|e| e.to_string())?;
                    let status = resp.status().as_u16();
                    let text = resp.text().await.unwrap_or_default();
                    let _ = env;
                    Ok::<Value, String>(json!({"status": status, "body": text}))
                });
                match result {
                    Ok(v) => write_json(&mut caller, v),
                    Err(e) => write_json(&mut caller, json!({"error": e})),
                }
            },
        )
        .map_err(|e| AppError::internal(format!("host http: {e}")))?;

    linker
        .func_wrap(
            "host",
            "fs_read",
            |mut caller: Caller<'_, HostCtx>, path_ptr: i32, path_len: i32| -> i64 {
                let path = read_string(&mut caller, path_ptr, path_len).unwrap_or_default();
                run_fs(&mut caller, "read", json!({"path": path}))
            },
        )
        .map_err(|e| AppError::internal(format!("host fs_read: {e}")))?;

    linker
        .func_wrap(
            "host",
            "fs_list",
            |mut caller: Caller<'_, HostCtx>, path_ptr: i32, path_len: i32| -> i64 {
                let path = read_string(&mut caller, path_ptr, path_len).unwrap_or_default();
                run_fs(&mut caller, "list", json!({"path": path}))
            },
        )
        .map_err(|e| AppError::internal(format!("host fs_list: {e}")))?;

    linker
        .func_wrap(
            "host",
            "fs_write",
            |mut caller: Caller<'_, HostCtx>,
             path_ptr: i32,
             path_len: i32,
             content_ptr: i32,
             content_len: i32|
             -> i64 {
                let caps = caller.data().caps.clone();
                let writable = caps
                    .get("fs")
                    .and_then(|f| f.get("write"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                if !writable {
                    return write_json(
                        &mut caller,
                        json!({"error": "fs write capability not granted"}),
                    );
                }
                let path = read_string(&mut caller, path_ptr, path_len).unwrap_or_default();
                let content =
                    read_string(&mut caller, content_ptr, content_len).unwrap_or_default();
                run_fs(
                    &mut caller,
                    "write",
                    json!({"path": path, "content": content}),
                )
            },
        )
        .map_err(|e| AppError::internal(format!("host fs_write: {e}")))?;

    linker
        .func_wrap(
            "host",
            "secret_get",
            |mut caller: Caller<'_, HostCtx>, name_ptr: i32, name_len: i32| -> i64 {
                let name = read_string(&mut caller, name_ptr, name_len).unwrap_or_default();
                let caps = caller.data().caps.clone();
                let allowed = caps
                    .get("secrets")
                    .and_then(Value::as_array)
                    .is_some_and(|a| a.iter().any(|v| v.as_str() == Some(name.as_str())));
                if !allowed {
                    return write_json(
                        &mut caller,
                        json!({"error": "secret not granted to this tool"}),
                    );
                }
                let Some(env) = caller.data().env.clone() else {
                    return write_json(&mut caller, json!({"error": "no execution context"}));
                };
                let handle = caller.data().handle.clone();
                match handle.block_on(env.secrets.get(&name)) {
                    Ok(value) => write_json(&mut caller, json!({"value": value})),
                    Err(e) => write_json(&mut caller, json!({"error": e.to_string()})),
                }
            },
        )
        .map_err(|e| AppError::internal(format!("host secret_get: {e}")))?;

    linker
        .func_wrap(
            "host",
            "log",
            |mut caller: Caller<'_, HostCtx>, ptr: i32, len: i32| {
                let msg = read_string(&mut caller, ptr, len).unwrap_or_default();
                tracing::debug!(target: "wasm_tool", "{msg}");
                let data = caller.data_mut();
                if data.logs.len() < 100 {
                    data.logs.push(msg.chars().take(500).collect());
                }
            },
        )
        .map_err(|e| AppError::internal(format!("host log: {e}")))?;

    Ok(linker)
}

fn run_fs(caller: &mut Caller<'_, HostCtx>, operation: &str, input: Value) -> i64 {
    let caps = caller.data().caps.clone();
    let Some(env) = caller.data().env.clone() else {
        return write_json(caller, json!({"error": "no execution context"}));
    };
    let Some(tool) = fs_tool(&caps, operation) else {
        return write_json(
            caller,
            json!({"error": "fs capability not declared (roots missing)"}),
        );
    };
    let handle = caller.data().handle.clone();
    match handle.block_on(tool.execute(&env, input)) {
        Ok(v) => write_json(caller, v),
        Err(e) => write_json(caller, json!({"error": e.to_string()})),
    }
}

fn fs_tool(caps: &Value, operation: &str) -> Option<FileTool> {
    let fs = caps.get("fs")?;
    let root = fs.get("roots")?.as_array()?.first()?.as_str()?.to_string();
    let mut config = json!({
        "operation": operation,
        "root": root,
        "max_file_bytes": 1_048_576
    });
    if let Some(g) = fs.get("allow_globs") {
        config["allow_globs"] = g.clone();
    }
    if let Some(g) = fs.get("deny_globs") {
        config["deny_globs"] = g.clone();
    }
    Some(FileTool {
        name: format!("host.fs.{operation}"),
        description: String::new(),
        parameters_schema: json!({"type": "object"}),
        config,
    })
}

fn host_allowed(caps: &Value, url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return false;
    }
    let Some(host) = parsed.host_str() else {
        return false;
    };
    let Some(hosts) = caps
        .get("http")
        .and_then(|h| h.get("allow_hosts"))
        .and_then(Value::as_array)
    else {
        return false;
    };
    hosts.iter().filter_map(Value::as_str).any(|pattern| {
        if let Some(suffix) = pattern.strip_prefix("*.") {
            let host = host.to_ascii_lowercase();
            let suffix = suffix.to_ascii_lowercase();
            host == suffix || host.ends_with(&format!(".{suffix}"))
        } else {
            host.eq_ignore_ascii_case(pattern)
        }
    })
}

fn read_string(caller: &mut Caller<'_, HostCtx>, ptr: i32, len: i32) -> Option<String> {
    if ptr < 0 || !(0..=4 * 1024 * 1024).contains(&len) {
        return None;
    }
    let memory = caller.get_export("memory")?.into_memory()?;
    let mut buf = vec![0u8; len as usize];
    memory.read(&*caller, ptr as usize, &mut buf).ok()?;
    String::from_utf8(buf).ok()
}

fn write_json(caller: &mut Caller<'_, HostCtx>, value: Value) -> i64 {
    let text = value.to_string();
    let bytes = text.as_bytes();
    let Some(alloc) = caller.data().alloc.clone() else {
        return 0;
    };
    let Ok(ptr) = alloc.call(&mut *caller, bytes.len() as i32) else {
        return 0;
    };
    let Some(memory) = caller.get_export("memory").and_then(|e| e.into_memory()) else {
        return 0;
    };
    if memory.write(&mut *caller, ptr as usize, bytes).is_err() {
        return 0;
    }
    ((ptr as i64) << 32) | bytes.len() as i64
}
