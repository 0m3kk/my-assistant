#!/usr/bin/env bash
# Backend `TOOL_BUILDER_CMD` entrypoint.
#
# The backend writes <tmp>/source.txt and expects <tmp>/out.wasm. This script
# runs the disposable builder container with no network and no secrets.
#
# Set in backend/.env (absolute path):
#   TOOL_BUILDER_CMD=/absolute/path/to/scripts/build-tool.sh
set -euo pipefail

SRC="${1:?usage: build-tool.sh <source-file> <output-wasm>}"
OUT="${2:?usage: build-tool.sh <source-file> <output-wasm>}"
DIR="$(cd "$(dirname "$SRC")" && pwd)"
BASE="$(basename "$SRC")"
OUTBASE="$(basename "$OUT")"

docker run --rm --network none \
  -v "$DIR":/work \
  -w /work \
  assistant-tool-builder \
  "/work/$BASE" "/work/$OUTBASE"
