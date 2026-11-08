# WASM tool ABI

A `wasm` tool is a single Rust source file compiled to `wasm32-wasip1` by the
sandboxed builder. The runtime uses **Wasmtime** with default-deny capabilities.

## Exports (required)

```rust
#[no_mangle]
pub extern "C" fn alloc(len: usize) -> *mut u8;

#[no_mangle]
pub extern "C" fn run(input_ptr: *const u8, input_len: usize) -> u64;
```

- `alloc` allocates `len` bytes in guest memory. The host uses it to write the
  input JSON and host-function results.
- `run` receives UTF-8 JSON `input` and returns a packed pointer:
  `(output_ptr as u64) << 32 | output_len as u64`.
- The output must be UTF-8 JSON (the host parses it; non-JSON is returned as a
  string).
- `memory` must be exported (the default for `cdylib` Rust modules).

## Host imports (`module "host"`)

Every `-> u64` returns `(ptr << 32) | len` pointing to a JSON string written
into guest memory via `alloc`.

```text
http(method_ptr, method_len, url_ptr, url_len,
     headers_ptr, headers_len, body_ptr, body_len) -> u64
fs_read(path_ptr, path_len) -> u64
fs_write(path_ptr, path_len, content_ptr, content_len) -> u64
fs_list(path_ptr, path_len) -> u64
secret_get(name_ptr, name_len) -> u64
log(ptr, len)
```

- `http` is allowed only for hosts declared in `capabilities.http.allow_hosts`;
  redirects are disabled; the result is `{"status": 200, "body": "..."}` or
  `{"error": "..."}`.
- `fs_read` / `fs_list` / `fs_write` use `capabilities.fs` (`roots`,
  `allow_globs`, `deny_globs`, `write`). `fs_write` requires `write: true`.
- `secret_get` is allowed only for names in `capabilities.secrets`; the value
  never enters model context.
- `log` is captured in the execution trace (bounded).

## Example

```rust
use std::alloc::{alloc as rust_alloc, Layout};

#[no_mangle]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    if len == 0 { return std::ptr::NonNull::dangling().as_ptr(); }
    unsafe { rust_alloc(Layout::from_size_align(len, 1).unwrap()) }
}

#[no_mangle]
pub extern "C" fn run(ptr: *const u8, len: usize) -> u64 {
    let input = unsafe { std::slice::from_raw_parts(ptr, len) };
    let value: serde_json::Value =
        serde_json::from_slice(input).unwrap_or(serde_json::Value::Null);
    let out = serde_json::json!({"received": value}).to_string().into_bytes();
    let out_ptr = alloc(out.len());
    unsafe { std::ptr::copy_nonoverlapping(out.as_ptr(), out_ptr, out.len()); }
    ((out_ptr as u64) << 32) | out.len() as u64
}
```

## Calling a host function

```rust
extern "C" {
    fn secret_get(name_ptr: *const u8, name_len: usize) -> u64;
}

fn host_string(packed: u64) -> String {
    let ptr = (packed >> 32) as *const u8;
    let len = packed as u32 as usize;
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    String::from_utf8_lossy(bytes).into_owned()
}

let name = b"github_token";
let result = unsafe { host_string(secret_get(name.as_ptr(), name.len())) };
// result is JSON: {"value": "..."} or {"error": "..."}
```

## Limits

- Wall-clock timeout: `timeout_ms` (epoch interruption).
- Memory: `memory_limit_mb` (StoreLimits).
- Fuel: a generous fixed cap (secondary safety).
- Output: 8 MB cap before the generic tool output cap.
- No filesystem/network unless the capability is declared and approved.
