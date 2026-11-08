# Architecture

## Tổng quan

```
┌──────────────────────────────┐        ┌──────────────────────────────────────────────┐
│  Frontend (Vite + React SPA) │  SSE   │  Backend (Rust + Axum)                        │
│                              │◄───────│                                              │
│  useChat (AI SDK v5)         │  REST  │  api/ ── service/ ── repo/ (SQLx) ── Postgres │
│  DefaultChatTransport        │◄───────│                          │                    │
│  AI Elements (shadcn)        │        │            runtime/ ─────┤                    │
│   • Chat / Message           │        │                │                              │
│   • Tool / Confirmation      │        │        aistream/ (UI Message Stream)          │
│   • Reasoning / Sources      │        │                │                              │
│  part renderer registry      │        │        tools/ (builtin | http | file | wasm)  │
│  tool renderers registry     │        │        llm/ (Provider trait → openaicompat)   │
│  Feedback / Proposal cards   │        │        secrets/ (AES-256-GCM + resolver)      │
└──────────────────────────────┘        └──────────────────────────────────────────────┘
```

## Luồng chat (SSE)

1. **Client pre-allocates conversation id** (uuid v4): mỗi conversation có một `Chat` instance
   do `lib/ai/chat-manager.ts` quản lý ở module level (sống ngoài component lifecycle —
   chuyển conversation / remount không bao giờ abort stream đang chạy; `useChat({ chat })`
   externally-managed). Backend **materialize** row trên tin nhắn đầu tiên (create-if-absent,
   ownership-check khi đã tồn tại), POST `/api/chat` body `{conversation_id, agent_slug?, model_id, messages}`.
2. Handler (service/`chat`):
   - Tìm/tạo conversation (run guard: 1 run active — `conversations.status` + DashMap in-memory)
   - Load agent + model + provider + tool set (agent_tools)
   - Tạo row `runs` (stable run id cho feedback), lưu user message
3. `runtime/` chạy loop:
   - Assemble: system prompt + history + tool definitions
   - Emit `data-run` (`{runId, agentId, agentSlug, modelId}`) — persisted into the assistant
     message parts so feedback survives reloads
   - `llm::Provider::stream_chat` → UI Message Stream chunks → SSE
   - Tool call: approval gate → `ToolRuntime` (scope → timeout → output cap → resolve
     `{{secret:}}` → execute → `tool_executions` + audit) → loop lại (tối đa `max_tool_calls`)
   - Kết thúc: persist assistant parts + `runs.status/finished_at` + usage log
4. Frontend render parts: `text` → Markdown, `tool-*` → Tool pattern + tool renderers registry,
   `data-*` → part renderers registry (`data-run` is hidden transport metadata).

### Approval flow

- Tool `requires_approval` chưa có decision: ghi row `approvals` (pending) → stream chunk
  `tool-approval-request` → kết thúc turn.
- File tool truy cập **ngoài root/allow_globs**: runtime trả `ToolError::OutOfRoot`; engine tạo
  approval `reason='out_of_root'` + `requested_resource`, kết thúc turn.
- UI Confirmation: card hiển thị tool/input/resource. Với out-of-root có 3 lựa chọn:
  **Deny / Allow once / Allow + propose update**. Lựa chọn cuối tạo một `change_proposals`
  pending để admin review việc mở rộng scope.
- Backend đối chiếu ledger `approvals` (chống giả mạo từ client) → execute → stream tiếp.
- Cancel: client abort hoặc `POST /api/chat/cancel` → `CancellationToken` → persist partial.

### UI Message Stream protocol

Chunk types mirror `UIMessageChunk` của package `ai`: `start`, `start-step`,
`text-start|delta|end`, `reasoning-*`, `tool-input-available`, `tool-output-available|error`,
`tool-approval-request`, `data-{name}`, `finish-step`, `finish`, `error`. Encoder (`aistream/`)
transport-agnostic: chunk structs serde + adapter SSE. Golden tests khoá format.

## Hệ tool

`trait Tool` + registry động. Mọi tool có cùng policy (scope, timeout, output cap,
requires_approval override theo agent, audit, `tool_executions`). **Không có terminal/shell/exec**
— đây là ràng buộc cứng: không tồn tại tool kind nào chạy lệnh hay nhận URL/command tùy ý.

| Kind | Cơ chế | Ví dụ |
|---|---|---|
| `builtin` | Code Rust biên dịch trong backend | `workspace.*`, `current_time`, `web_search`, `save_artifact`, `meta.*` |
| `http` | Một endpoint cố định: `base_url` + `path_template` + query/body/headers + `{{secret:}}` | `github.get_pr_diff`, `github.submit_review` |
| `file` | Một operation trên root cố định: read/list/search/write/edit + allow/deny globs | `repo.read_file`, `repo.apply_patch` |
| `wasm` | Source → compiled artifact; Wasmtime sandbox + capability host functions | transform/parse chuyên biệt |
| `native` | Binary tin cậy do admin upload; chạy subprocess JSON stdin/stdout, không qua shell | hiếm, ưu tiên wasm |
| `mcp` | Reserved | — |

**Tool tự chứa config** — không có connection/grant entity trung gian. Secret được khai báo trực
tiếp trong config (`{{secret:NAME}}` hoặc capability `secrets`), hiển thị rõ khi review, và chỉ
resolve tại execute time.

`web_search` (builtin) gọi SearXNG `GET /search?format=json`; `base_url` lấy từ `tools.config`
(Admin → Tools) hoặc env `SEARXNG_URL`. Output chuẩn hoá
`{provider, query, results[{title,url,snippet,published_at}], answers?, suggestions?}`; frontend
render danh sách Sources trong trace của tool.

### Scope & blast radius

- `http`: host cố định; path/query/body chỉ từ schema params; không có tham số URL đầy đủ;
  redirect bị chặn.
- `file`: root canonicalize, chặn `..`, symlink escape; allow/deny globs; size cap; path ngoài
  root → approval (không silent escape).
- `wasm`: default-deny capability; CPU/memory/output cap; không ambient fs/network.
- Không tool nào được gọi tool khác. Agent orchestrate nhiều tool hẹp.

## Feedback

- User bấm 👍/👎 (+ note) trên từng tool run và từng assistant message (agent run).
- `feedback` unique theo `(user_id, subject_key)`; `subject_key = tool:{conversation}:{call_id}`
  hoặc `agent:{run_id}`.
- Admin có feedback inbox trong `/studio`; "Improve" mở meta-agent tương ứng với feedback id.

## Proposals (human-in-the-loop)

Meta-agent **chỉ đọc và propose**, không có quyền ghi `tools`/`agents`:

| slug | vai trò |
|---|---|
| `tool-writer` | thiết kế tool mới |
| `tool-improver` | đọc feedback/executions, propose cập nhật tool |
| `agent-writer` | thiết kế agent mới |
| `agent-improver` | đọc feedback, propose cập nhật agent |

Flow: `meta.propose_*` → `change_proposals` (pending) → admin review/sửa payload → apply.
Apply chạy transaction: tạo/cập nhật resource, tăng `revision`, ghi `tool_versions`/`agent_versions`,
mark feedback addressed, audit. `wasm` tool chỉ enable sau khi build thành công.

Meta tools: `meta.list_tools/get_tool`, `meta.list_agents/get_agent`, `meta.list_feedback`,
`meta.list_executions`, `meta.validate_tool`, `meta.test_tool`, `meta.propose_tool`,
`meta.propose_agent`, `meta.list_proposals`.

## Programmable tools (wasm)

- Source + manifest (language, capabilities, timeout/memory) nằm trong `tool_builds`.
- Admin bấm **Build** (`POST /admin/tools/{id}/build`): builder chạy trong container disposable
  (Rust toolchain, no network, no secrets), tạo `.wasm`, lưu storage + sha256.
- Build **không** là tool của agent — model không bao giờ có compiler/terminal.
- Runtime: Wasmtime, capability host functions (`http`, `fs`, `secrets`, `log`), default-deny.
- Artifact hash + revision được lưu; tool chỉ `enabled` sau build thành công.

## Storage — attachments, artifacts, workspace

| Loại | Nguồn | Nơi lưu | Hiển thị |
|---|---|---|---|
| **Attachment** | User upload (`POST /api/files`) | Storage trait | File card trong message; model thấy ảnh/text |
| **Artifact** | `save_artifact` | Storage trait | Download card trong tool output |
| **Workspace file** | `workspace.write` / file tool | `DATA_DIR/workspace/<conv>/` (PathGuard) | Workspace panel |

`Storage` trait: `put/get/delete`; backend `local` hoặc `s3` (`STORAGE_BACKEND`).

## Secrets

- Bảng `secrets` — AES-256-GCM (`APP_SECRET`), version, last_used_at.
- Tham chiếu `{{secret:NAME}}` trong http tool config / capabilities / MCP env.
- Resolve **chỉ tại execute time**; không bao giờ vào LLM context, logs, API responses, errors.
- `required_secrets` / capability `secrets` → hiển thị khi review; runtime fail rõ ràng nếu thiếu.
- Delete: ref-check (chặn + liệt kê referencer).

## Hai chế độ chạy

| | `APP_MODE=server` | `APP_MODE=local` |
|---|---|---|
| Triển khai | Docker compose | Binary/compose trên máy user |
| `workspace.*` | Sandbox workspace | Sandbox workspace |
| `file` tool | Chỉ `@workspace` (chặn absolute root) | Root tuyệt đối do admin cấu hình |
| Tools ưu tiên | `http`, `workspace.*`, `wasm` | `file`, `http`, `wasm` |
| Code exec | Chỉ wasm sandbox / builder container | wasm sandbox / builder container |

## Run lifecycle & data

- `runs`: 1 row per turn; `assistant_message_id`, status (`running|completed|cancelled|failed|approval_pending`).
- `tool_executions`: input/output/status/duration/resources_used; feedback anchor + improver signal.
- `messages.run_id` + `data-run` part: stable identity across reloads.
- `audit_logs`: approvals, tool executions, proposal apply/reject, secret changes.

## Production topology

Reverse proxy (Nginx/Caddy — TLS, HTTP/2) → Axum (static dist + API + SSE). SSE cần LB không
buffer; scaling đa node là phase sau (Redis pubsub). Health: `GET /api/health`. Builder container
chạy tách biệt, không network, không secrets.

## Quy ước code

- Backend: module boundary `api → service → repo`; types chia sẻ ở `domain/`; lỗi `thiserror` →
  `IntoResponse` (RFC 9457); query SQLx compile-time checked (offline `.sqlx` qua
  `cargo sqlx prepare`); `cargo clippy -D warnings` + `cargo fmt` bắt buộc.
- Frontend: `components/ui` (shadcn base) / `components/ai-elements` (patterns copy) /
  `components/chat|admin|studio` (feature); API client typed; i18n mọi string qua `t()`.
- Migrations: **một** baseline `0001_init.sql` (greenfield); reset dev DB bằng `make db-reset`.
