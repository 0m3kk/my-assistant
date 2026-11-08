#!/usr/bin/env bash
# Build a single-file Rust wasm tool.
#
# Usage: entrypoint.sh <source-file> <output-wasm>
# The source file must define the tool ABI (see docs/TOOL_ABI.md):
#   #[no_mangle] pub extern "C" fn alloc(len: usize) -> *mut u8
#   #[no_mangle] pub extern "C" fn run(ptr: *const u8, len: usize) -> u64
set -euo pipefail

SRC="${1:?usage: entrypoint.sh <source-file> <output-wasm>}"
OUT="${2:?usage: entrypoint.sh <source-file> <output-wasm>}"

WORK="/work/build"
rm -rf "$WORK"
mkdir -p "$WORK/src"

cat > "$WORK/Cargo.toml" <<'EOF'
[package]
name = "tool"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
serde_json = "1"
EOF

cp "$SRC" "$WORK/src/lib.rs"
cd "$WORK"
cargo build --release --offline --target wasm32-wasip1
cp "$WORK/target/wasm32-wasip1/release/tool.wasm" "$OUT"
