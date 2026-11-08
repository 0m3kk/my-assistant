// Example wasm tool source. Copy this into a `wasm` tool's `config.source`.
//
// The builder compiles this as a cdylib for wasm32-wasip1.
use std::alloc::{alloc as rust_alloc, Layout};

#[no_mangle]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    if len == 0 {
        return std::ptr::NonNull::dangling().as_ptr();
    }
    unsafe { rust_alloc(Layout::from_size_align(len, 1).unwrap()) }
}

#[no_mangle]
pub extern "C" fn run(ptr: *const u8, len: usize) -> u64 {
    let input = unsafe { std::slice::from_raw_parts(ptr, len) };
    let value: serde_json::Value =
        serde_json::from_slice(input).unwrap_or(serde_json::Value::Null);
    let out = serde_json::json!({
        "received": value,
        "note": "replace this example with the tool logic"
    })
    .to_string()
    .into_bytes();
    let out_ptr = alloc(out.len());
    unsafe { std::ptr::copy_nonoverlapping(out.as_ptr(), out_ptr, out.len()); }
    ((out_ptr as u64) << 32) | out.len() as u64
}
