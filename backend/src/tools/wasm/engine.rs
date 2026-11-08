//! Wasmtime engine + module cache for `wasm` tools.
//!
//! ABI: the guest exports `memory`, `alloc(len) -> ptr`, and
//! `run(input_ptr, input_len) -> u64` where the return packs
//! `(output_ptr << 32) | output_len`. Host capabilities are imported from the
//! `"host"` module (see [`host`]).

use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use futures::StreamExt;
use serde_json::{Value, json};
use wasmtime::{Config, Engine, Module, Store, StoreLimitsBuilder};
use wasmtime_wasi::WasiCtxBuilder;

use super::host::{self, HostCtx};
use crate::error::{AppError, AppResult};
use crate::storage::Storage;
use crate::tools::ToolExecContext;

pub struct WasmEngine {
    engine: Engine,
    modules: Arc<DashMap<String, Module>>,
}

impl WasmEngine {
    pub fn new() -> AppResult<Arc<Self>> {
        let mut config = Config::new();
        config.epoch_interruption(true);
        config.consume_fuel(true);
        let engine =
            Engine::new(&config).map_err(|e| AppError::internal(format!("wasm engine: {e}")))?;
        Ok(Arc::new(Self {
            engine,
            modules: Arc::new(DashMap::new()),
        }))
    }

    pub async fn execute(
        &self,
        ctx: &ToolExecContext,
        config: &Value,
        input: Value,
    ) -> AppResult<Value> {
        let key = config
            .get("artifact_storage_key")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AppError::bad_request("wasm tool has no built artifact; build it from the admin UI")
            })?;
        let sha = config
            .get("artifact_sha256")
            .and_then(Value::as_str)
            .unwrap_or(key);
        let module = self.load_module(&ctx.storage, key, sha).await?;
        let timeout_ms = config
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(10_000);
        let memory_mb = config
            .get("memory_limit_mb")
            .and_then(Value::as_u64)
            .unwrap_or(64);
        let caps = config
            .get("capabilities")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let env = Some(ctx.clone());
        let engine = self.engine.clone();
        let handle = tokio::runtime::Handle::current();
        let module = module.clone();
        tokio::task::spawn_blocking(move || {
            run_module(
                &engine, &module, input, timeout_ms, memory_mb, env, caps, handle,
            )
        })
        .await
        .map_err(|e| AppError::internal(format!("wasm task failed: {e}")))?
    }

    async fn load_module(
        &self,
        storage: &Arc<dyn Storage>,
        key: &str,
        sha: &str,
    ) -> AppResult<Module> {
        if let Some(m) = self.modules.get(sha) {
            return Ok(m.clone());
        }
        let mut stream = storage
            .get(key)
            .await
            .map_err(|e| AppError::internal(format!("cannot load wasm artifact: {e}")))?;
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk =
                chunk.map_err(|e| AppError::internal(format!("cannot read wasm artifact: {e}")))?;
            bytes.extend_from_slice(&chunk);
            if bytes.len() > 64 * 1024 * 1024 {
                return Err(AppError::payload_too_large("wasm artifact is too large"));
            }
        }
        let module = Module::from_binary(&self.engine, &bytes)
            .map_err(|e| AppError::bad_request(format!("invalid wasm artifact: {e}")))?;
        self.modules.insert(sha.to_string(), module.clone());
        Ok(module)
    }
}

/// Execute a module synchronously (called from `spawn_blocking`).
#[allow(clippy::too_many_arguments)]
pub fn run_module(
    engine: &Engine,
    module: &Module,
    input: Value,
    timeout_ms: u64,
    memory_mb: u64,
    env: Option<ToolExecContext>,
    caps: Value,
    handle: tokio::runtime::Handle,
) -> AppResult<Value> {
    let input_json = serde_json::to_string(&input)?;
    let limits = StoreLimitsBuilder::new()
        .memory_size((memory_mb.max(8) as usize) * 1024 * 1024)
        .build();
    let mut store = Store::new(
        engine,
        HostCtx {
            wasi: WasiCtxBuilder::new().build_p1(),
            env,
            caps,
            limits,
            alloc: None,
            handle,
            logs: Vec::new(),
        },
    );
    store.limiter(|d| &mut d.limits);
    store
        .set_fuel(10_000_000_000)
        .map_err(|e| AppError::internal(format!("wasm fuel: {e}")))?;
    store.set_epoch_deadline(1);

    let linker = host::build_linker(engine)?;
    let instance = linker
        .instantiate(&mut store, module)
        .map_err(|e| AppError::bad_request(format!("wasm instantiate: {e}")))?;
    let alloc = instance
        .get_typed_func::<i32, i32>(&mut store, "alloc")
        .map_err(|_| AppError::bad_request("wasm tool must export alloc(len) -> ptr"))?;
    store.data_mut().alloc = Some(alloc.clone());
    let run = instance
        .get_typed_func::<(i32, i32), i64>(&mut store, "run")
        .map_err(|_| AppError::bad_request("wasm tool must export run(ptr, len) -> u64"))?;

    let bytes = input_json.as_bytes();
    let ptr = alloc
        .call(&mut store, bytes.len() as i32)
        .map_err(|e| AppError::bad_request(format!("wasm alloc failed: {e}")))?;
    let memory = instance
        .get_memory(&mut store, "memory")
        .ok_or_else(|| AppError::bad_request("wasm tool must export memory"))?;
    memory
        .write(&mut store, ptr as usize, bytes)
        .map_err(|e| AppError::bad_request(format!("wasm input write failed: {e}")))?;

    // Wall-clock deadline via epoch interruption (fuel is a secondary cap).
    let engine_watch = engine.clone();
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let timeout = Duration::from_millis(timeout_ms.max(1));
    let watchdog = std::thread::spawn(move || {
        if rx.recv_timeout(timeout).is_err() {
            engine_watch.increment_epoch();
        }
    });
    let result = run.call(&mut store, (ptr, bytes.len() as i32));
    let _ = tx.send(());
    let _ = watchdog.join();

    let packed = result.map_err(|e| AppError::bad_request(format!("wasm trap: {e}")))?;
    let out_ptr = ((packed as u64) >> 32) as u32;
    let out_len = (packed as u64 & 0xffff_ffff) as u32;
    if out_len > 8 * 1024 * 1024 {
        return Err(AppError::payload_too_large("wasm output is too large"));
    }
    let mut out = vec![0u8; out_len as usize];
    memory
        .read(&store, out_ptr as usize, &mut out)
        .map_err(|e| AppError::bad_request(format!("wasm output read failed: {e}")))?;
    let text = String::from_utf8(out)
        .map_err(|_| AppError::bad_request("wasm output is not valid UTF-8"))?;
    Ok(serde_json::from_str(&text).unwrap_or(Value::String(text)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executes_pure_wasm_module() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let handle = rt.handle().clone();
        let engine = WasmEngine::new().unwrap();
        let wat = r#"
        (module
          (memory (export "memory") 1)
          (data (i32.const 1024) "{\"ok\":true}")
          (func (export "alloc") (param i32) (result i32) (i32.const 2048))
          (func (export "run") (param i32 i32) (result i64)
            i64.const 1024
            i64.const 32
            i64.shl
            i64.const 11
            i64.or))
        "#;
        let bytes = wat::parse_str(wat).unwrap();
        let module = Module::from_binary(&engine.engine, &bytes).unwrap();
        let out = run_module(
            &engine.engine,
            &module,
            json!({"n": 1}),
            2_000,
            32,
            None,
            json!({}),
            handle,
        )
        .unwrap();
        assert_eq!(out["ok"], json!(true));
    }
}
