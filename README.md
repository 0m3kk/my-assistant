# My Assistant

AI assistant application — Rust (Axum) backend, React (Vite + shadcn AI Elements) frontend, PostgreSQL.

- Docs: [ARCHITECTURE.md](docs/ARCHITECTURE.md) · [ROADMAP.md](docs/ROADMAP.md) · [TOOL_ABI.md](docs/TOOL_ABI.md)

## Quick start (dev)

```bash
# 1. Postgres + env
cp backend/.env.example backend/.env          # adjust if needed
make docker-postgres
make docker-searxng                           # SearXNG for web_search (:8080)

# Existing dev DB from an older schema? Reset it (greenfield single baseline):
make db-reset

# 2. Backend (migrations run automatically at startup)
make dev-backend                              # http://localhost:3000

# 3. Frontend
cd frontend && pnpm install && pnpm dev      # http://localhost:5173 (proxy /api → :3000)
```

## Users

Public sign-up is **disabled by default** — accounts are managed from the CLI:

```bash
cd backend
cargo run -- add-user --email admin@example.com           # password prompted (masked)
cargo run -- list-users [--json]
cargo run -- set-role --email bob@example.com --role admin
cargo run -- delete-user --email bob@example.com [--yes]  # refuses to remove the last admin
assistant --help                                          # in release/Docker builds
```

- The first user created defaults to `admin`; later ones to `user` (override with `--role admin|user`).
- `delete-user` / `set-role` accept `--email` or `--id`; both refuse to remove the last remaining admin.
- Re-enable public sign-up with `REGISTRATION_ENABLED=true`.
- In Docker: `docker compose -f deploy/docker-compose.yml exec app assistant --help`

## Tools, feedback & Studio

- Tools are **self-contained** (`http`, `file`, or `wasm`) — there are no connection/grant entities.
  Each tool carries its own base URL/root, allow/deny globs, secret refs, and `requires_approval`.
- There is **no terminal/shell/exec tool**. HTTP hosts are fixed, file access is scoped, and
  out-of-root file access asks for approval (Deny / Allow once / Allow + propose update).
- Users leave 👍/👎 feedback on tool runs and agent runs; admins open **Studio** (`/studio`) to
  triage feedback, chat with the meta-agents (`tool-writer`, `tool-improver`, `agent-writer`,
  `agent-improver`), and review/apply proposals. Nothing is saved until an admin applies a
  proposal.
- Programmable tools: source + manifest → admin-triggered build in a disposable container →
  WASM artifact stored + hashed. The build is never exposed as an agent tool.
- To enable wasm builds: run `make tool-builder` once, then set
  `TOOL_BUILDER_CMD=/absolute/path/to/scripts/build-tool.sh` in `backend/.env`.
  Admin → Tools → Details shows build status/log, a restricted test run, and version history with
  restore proposals. ABI: [docs/TOOL_ABI.md](docs/TOOL_ABI.md).

## Production

```bash
JWT_SECRET=$(openssl rand -hex 32) APP_SECRET=$(openssl rand -hex 32) make docker-up
# → http://localhost:3000 (backend serves the built SPA)
```

## Notes

- UI font: **Geist Mono** everywhere (self-hosted via `@fontsource-variable/geist-mono`)
- Switching conversations never interrupts an in-flight reply — chat streams are owned by
  session-level `Chat` instances and the backend completes runs even if the client disconnects
  (explicit Stop/cancel is the only way to halt a run)
- Every turn creates a `runs` row and emits a persisted `data-run` part, so feedback is anchored
  to the exact run across reloads.

## Modes

- `APP_MODE=server` (default in Docker): remote work over HTTP; `workspace.*` is sandboxed to
  `DATA_DIR/workspace/<conversation-id>/`, and `file` tools may only use `@workspace`.
- `APP_MODE=local`: `file` tools may use an absolute root configured on the tool
  (Admin → Tools); paths are still root-contained with allow/deny globs.
