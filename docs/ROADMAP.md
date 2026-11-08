# Roadmap

Lộ trình phát triển sản phẩm. Phase 0–1 đã xong; các phase sau sẽ được chi tiết hoá khi bắt đầu.

## Legend

- ✅ đã có · 🔜 tiếp theo · 🧭 sau này

## Phase 0 — Scaffold ✅

- Monorepo: `backend/` (Rust + Axum + SQLx), `frontend/` (Vite + React + shadcn), `deploy/`
- Config + `APP_MODE` (`server` | `local`), DATA_DIR
- Một baseline schema duy nhất (`0001_init.sql`): users, preferences, secrets, providers, models,
  agents, tools, agent_tools, conversations, messages, runs, tool_executions, feedback,
  change_proposals, tool_builds, tool_versions, agent_versions, files, approvals, usage, audit
- JWT auth (register/login/me, Argon2id)
- Static serving + SPA fallback
- i18n + Theme + Font scale

## Phase 1 — Vertical slice ✅

- Chat E2E: `POST /api/chat` SSE theo UI Message Stream protocol của AI SDK v5
- `openaicompat` adapter + provider presets: glm, opencode, openai, openrouter, ollama, groq, custom
- Admin CRUD: providers, models, agents, tools, secrets
- Secrets: AES-256-GCM, `{{secret:NAME}}`, resolve tại execute time
- **Tool kinds**: `builtin` (`workspace.*`, `current_time`, `web_search`, `save_artifact`, `meta.*`),
  `http` (endpoint cố định + secret refs), `file` (root + globs + read/list/search/write/edit),
  `wasm` (programmable)
- **No terminal/shell/exec** — không tồn tại kind nào chạy lệnh
- **Runs + tool_executions**: stable run id (`data-run`), execution ledger, audit
- **Feedback**: 👍/👎 + note trên tool run và agent run; feedback inbox
- **Proposals (HITL)**: `change_proposals`; meta-agents `tool-writer`, `tool-improver`,
  `agent-writer`, `agent-improver`; admin review/edit/apply; revision + version snapshots
- **Approval**: per-tool `requires_approval`; file out-of-root → Deny / Allow once /
  Allow + propose scope update
- **Studio**: `/studio` meta-agent chats + proposal review + feedback inbox
- **WASM tools**: Wasmtime sandbox, capability host functions (`http`, `fs`, `secrets`, `log`),
  default-deny, fuel/epoch/memory/output limits, artifact hash + revision
- **Sandboxed builder**: `make tool-builder` (Rust toolchain, vendored serde_json, `--network none`,
  no secrets), `scripts/build-tool.sh`, `TOOL_BUILDER_CMD`; admin-triggered only, never an agent tool
- **Admin tool details**: build status/log, restricted test run (no secrets/writes),
  version history + restore proposals
- Frontend: Chat UI, tool renderers, feedback controls, proposal cards, admin pages, settings

## Phase 2 — Providers & tools mở rộng 🔜

- Native adapters: Anthropic Messages API, Google Gemini
- OpenAPI import → sinh `http` tools có scope cố định
- Builtin tools: `memory` (facts), `scheduler` (reminders), `notify` (email/telegram/webhook)
- Tool renderers: `web_search` → Sources, `file.*` → File Tree; artifact cards
- Generative UI (json-render): interactive actions, replay `currentTree`, mở rộng catalog
- Reasoning display khi model trả reasoning_content
- Usage/cost dashboard, rate limit per user
- File tools nâng cao: atomic multi-file patch, diff preview, conflict detection
- WASM builder: thêm AssemblyScript runtime (hiện builder chỉ build Rust)

## Phase 3 — Knowledge, agents & programmable tools 🧭

- ✅ Web search thật: SearXNG self-host (JSON API; `base_url` từ `SEARXNG_URL` hoặc
  tool config). Tavily/Serper: tùy chọn sau.
- RAG: ingestion + pgvector + retrieval tool + citations
- Subagents (scope riêng), Task/Plan patterns
- WASM capability mở rộng: `ctx.tools.call` composition, structured logs trong tool output
- Native trusted binary tools (admin upload, subprocess JSON protocol)
- **Không** shell/exec/code_exec trên host

## Phase 4 — Platform 🧭

- MCP client (`rmcp`): stdio/http server → tools tự động, namespace `server__tool`, env qua secrets
- OAuth2 social login (Google/GitHub)
- Teams / workspaces, sharing conversations
- Quotas + audit log UI
- Prompt templates library
- Conversation branching / checkpoints

## Phase 5 — Production hardening 🧭

- Envelope encryption + master key rotation; OS keychain (`keyring`) cho local mode
- OpenTelemetry export
- Horizontal scaling: Redis pubsub run state, sticky LB cho SSE
- PWA, i18n mở rộng, backup/DR runbook
- OpenAPI docs (utoipa) + Scalar UI

## Định hướng kỹ thuật dài hạn

- Tool kinds: `builtin` → `http` → `file` → `wasm` (programmable) → `native` (trusted) → `mcp`.
  Mọi tool tự chứa config; không có connection/grant entity trung gian.
- **Ràng buộc cứng**: không terminal/shell/exec/gateway URL tùy ý. Build/compile không phải tool
  của agent.
- Human-in-the-loop: meta-agent chỉ propose; admin apply; feedback + executions là tín hiệu cải tiến.
- Provider adapter contract: `trait Provider` — thêm nhà cung cấp mới không sửa runtime/aistream.
- UI Message Stream encoder transport-agnostic: SSE hôm nay, WebSocket nếu cần notifications.
