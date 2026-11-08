-- Single baseline schema (greenfield rewrite; no incremental migrations).
--
-- Design notes:
--   * tools are self-contained: config holds base_url/root/globs/secret refs/capabilities
--   * no terminal/shell/exec tool kind exists by construction
--   * runs + tool_executions anchor feedback and audits
--   * change_proposals is the human-in-the-loop gate for tools/agents
--   * tool_builds stores source + compiled wasm artifact metadata

CREATE EXTENSION IF NOT EXISTS pgcrypto;

-- ---------- Users & preferences ----------

CREATE TABLE users (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email         TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    name          TEXT NOT NULL DEFAULT '',
    role          TEXT NOT NULL DEFAULT 'user' CHECK (role IN ('admin', 'user')),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE user_preferences (
    user_id    UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    theme      TEXT NOT NULL DEFAULT 'system' CHECK (theme IN ('system', 'light', 'dark')),
    font_scale TEXT NOT NULL DEFAULT 'md' CHECK (font_scale IN ('sm', 'md', 'lg')),
    locale     TEXT NOT NULL DEFAULT 'en',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ---------- Secrets (AES-256-GCM encrypted at rest) ----------

CREATE TABLE secrets (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name            TEXT NOT NULL UNIQUE,
    description     TEXT NOT NULL DEFAULT '',
    value_encrypted BYTEA NOT NULL,
    nonce           BYTEA NOT NULL,
    version         INT  NOT NULL DEFAULT 1,
    last_used_at    TIMESTAMPTZ,
    created_by      UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ---------- LLM providers & models ----------

CREATE TABLE providers (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name       TEXT NOT NULL,
    preset     TEXT NOT NULL DEFAULT 'custom'
               CHECK (preset IN ('glm', 'opencode', 'openai', 'openrouter', 'ollama', 'groq', 'custom')),
    base_url   TEXT NOT NULL,
    secret_id  UUID REFERENCES secrets(id) ON DELETE SET NULL,
    status     TEXT NOT NULL DEFAULT 'enabled' CHECK (status IN ('enabled', 'disabled')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE models (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    provider_id      UUID NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
    api_name         TEXT NOT NULL,
    display_name     TEXT NOT NULL DEFAULT '',
    context_window   INT,
    capabilities     JSONB NOT NULL DEFAULT '{}'::jsonb, -- {tools, vision, reasoning}
    reasoning_effort TEXT CHECK (reasoning_effort IN ('low', 'medium', 'high', 'xhigh')),
    is_default       BOOLEAN NOT NULL DEFAULT FALSE,
    status           TEXT NOT NULL DEFAULT 'enabled' CHECK (status IN ('enabled', 'disabled')),
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (provider_id, api_name)
);

CREATE INDEX idx_models_provider ON models(provider_id);

-- ---------- Agents & tools ----------

CREATE TABLE agents (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name             TEXT NOT NULL,
    slug             TEXT NOT NULL UNIQUE,
    description      TEXT NOT NULL DEFAULT '',
    system_prompt    TEXT NOT NULL DEFAULT '',
    default_model_id UUID REFERENCES models(id) ON DELETE SET NULL,
    temperature      REAL,
    max_tokens       INT,
    max_tool_calls   INT NOT NULL DEFAULT 8,
    kind             TEXT NOT NULL DEFAULT 'chat' CHECK (kind IN ('chat', 'meta')),
    is_builtin       BOOLEAN NOT NULL DEFAULT FALSE,
    revision         INT NOT NULL DEFAULT 1,
    status           TEXT NOT NULL DEFAULT 'enabled' CHECK (status IN ('enabled', 'disabled')),
    created_by       UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE tools (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name              TEXT NOT NULL UNIQUE,
    display_name      TEXT NOT NULL DEFAULT '',
    description       TEXT NOT NULL DEFAULT '',
    -- 'wasm' = source -> compiled artifact; 'native' = trusted prebuilt binary (admin upload)
    kind              TEXT NOT NULL CHECK (kind IN ('builtin', 'http', 'file', 'wasm', 'native', 'mcp')),
    source_ref        TEXT NOT NULL DEFAULT '',
    config            JSONB NOT NULL DEFAULT '{}'::jsonb,
    parameters_schema JSONB NOT NULL DEFAULT '{"type":"object","properties":{}}'::jsonb,
    requires_approval BOOLEAN NOT NULL DEFAULT FALSE,
    is_meta           BOOLEAN NOT NULL DEFAULT FALSE,
    is_builtin        BOOLEAN NOT NULL DEFAULT FALSE,
    revision          INT NOT NULL DEFAULT 1,
    status            TEXT NOT NULL DEFAULT 'enabled' CHECK (status IN ('enabled', 'disabled')),
    created_by        UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE agent_tools (
    agent_id          UUID NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    tool_id           UUID NOT NULL REFERENCES tools(id) ON DELETE CASCADE,
    requires_approval BOOLEAN,
    config            JSONB,
    PRIMARY KEY (agent_id, tool_id)
);

CREATE TABLE agent_versions (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    agent_id    UUID NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    revision    INT NOT NULL,
    snapshot    JSONB NOT NULL,
    created_by  UUID REFERENCES users(id) ON DELETE SET NULL,
    proposal_id UUID,
    note        TEXT NOT NULL DEFAULT '',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (agent_id, revision)
);

CREATE TABLE tool_versions (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tool_id     UUID NOT NULL REFERENCES tools(id) ON DELETE CASCADE,
    revision    INT NOT NULL,
    snapshot    JSONB NOT NULL,
    created_by  UUID REFERENCES users(id) ON DELETE SET NULL,
    proposal_id UUID,
    note        TEXT NOT NULL DEFAULT '',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tool_id, revision)
);

-- Source + compiled artifact for code tools (one row per revision).
CREATE TABLE tool_builds (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tool_id     UUID NOT NULL REFERENCES tools(id) ON DELETE CASCADE,
    revision    INT NOT NULL,
    language    TEXT NOT NULL DEFAULT 'rust',
    source      TEXT NOT NULL,
    runtime     TEXT NOT NULL CHECK (runtime IN ('wasm', 'native')),
    status      TEXT NOT NULL DEFAULT 'pending'
                CHECK (status IN ('pending', 'building', 'built', 'failed')),
    storage_key TEXT,
    sha256      TEXT,
    size_bytes  BIGINT,
    build_log   TEXT,
    created_by  UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ,
    UNIQUE (tool_id, revision)
);

-- ---------- Conversations, runs & messages ----------

CREATE TABLE conversations (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    agent_id        UUID NOT NULL REFERENCES agents(id) ON DELETE RESTRICT,
    title           TEXT NOT NULL DEFAULT 'New conversation',
    model_id        UUID REFERENCES models(id) ON DELETE SET NULL,
    status          TEXT NOT NULL DEFAULT 'idle' CHECK (status IN ('idle', 'running')),
    last_message_at TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_conversations_user ON conversations(user_id, last_message_at DESC);

CREATE TABLE runs (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    conversation_id      UUID NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    agent_id             UUID NOT NULL REFERENCES agents(id) ON DELETE SET NULL,
    model_id             UUID REFERENCES models(id) ON DELETE SET NULL,
    -- FK added after messages exists (circular reference).
    assistant_message_id UUID,
    status               TEXT NOT NULL DEFAULT 'running'
                         CHECK (status IN ('running', 'completed', 'cancelled', 'failed', 'approval_pending')),
    started_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at          TIMESTAMPTZ
);

CREATE INDEX idx_runs_conversation ON runs(conversation_id, started_at DESC);

CREATE TABLE messages (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    conversation_id UUID NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    run_id          UUID REFERENCES runs(id) ON DELETE SET NULL,
    role            TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
    parts           JSONB NOT NULL DEFAULT '[]'::jsonb,  -- UIMessage.parts shape (AI SDK v5)
    model_id        UUID REFERENCES models(id) ON DELETE SET NULL,
    usage           JSONB,
    seq             INT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (conversation_id, seq)
);

CREATE INDEX idx_messages_conversation ON messages(conversation_id, seq);
CREATE INDEX idx_messages_run ON messages(run_id);

ALTER TABLE runs
    ADD CONSTRAINT runs_assistant_message_fk
    FOREIGN KEY (assistant_message_id) REFERENCES messages(id) ON DELETE SET NULL;

-- ---------- Tool executions (audit + feedback anchor) ----------

CREATE TABLE tool_executions (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    run_id          UUID REFERENCES runs(id) ON DELETE CASCADE,
    conversation_id UUID NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    tool_call_id    TEXT NOT NULL,
    tool_id         UUID REFERENCES tools(id) ON DELETE SET NULL,
    tool_name       TEXT NOT NULL,
    input           JSONB NOT NULL DEFAULT '{}'::jsonb,
    output          JSONB,
    status          TEXT NOT NULL CHECK (status IN ('ok', 'error', 'denied', 'timeout')),
    error_text      TEXT,
    duration_ms     INT,
    resources_used  JSONB, -- hosts/paths touched (audit + improver signal)
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_tool_exec_run ON tool_executions(run_id);
CREATE INDEX idx_tool_exec_tool ON tool_executions(tool_id, created_at DESC);
CREATE INDEX idx_tool_exec_conversation ON tool_executions(conversation_id, created_at DESC);

-- ---------- Approvals (human-in-the-loop ledger) ----------

CREATE TABLE approvals (
    id                 UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    conversation_id    UUID NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    tool_call_id       TEXT NOT NULL,
    tool_name          TEXT NOT NULL,
    input              JSONB NOT NULL DEFAULT '{}'::jsonb,
    reason             TEXT NOT NULL DEFAULT 'requires_approval'
                       CHECK (reason IN ('requires_approval', 'out_of_root')),
    requested_resource TEXT,
    status             TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'approved', 'denied')),
    decision_mode      TEXT CHECK (decision_mode IN ('once', 'propose_update')),
    decided_at         TIMESTAMPTZ,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (conversation_id, tool_call_id)
);

CREATE INDEX idx_approvals_pending ON approvals(conversation_id) WHERE status = 'pending';

-- ---------- Change proposals (human-in-the-loop gate) ----------

CREATE TABLE change_proposals (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    resource            TEXT NOT NULL CHECK (resource IN ('tool', 'agent')),
    action              TEXT NOT NULL CHECK (action IN ('create', 'update')),
    target_id           UUID,
    status              TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'applied', 'rejected')),
    title               TEXT NOT NULL DEFAULT '',
    rationale           TEXT NOT NULL DEFAULT '',
    payload             JSONB NOT NULL,
    before              JSONB,
    feedback_ids        UUID[] NOT NULL DEFAULT '{}',
    created_by_user_id  UUID REFERENCES users(id) ON DELETE SET NULL,
    created_by_agent_id UUID REFERENCES agents(id) ON DELETE SET NULL,
    conversation_id     UUID REFERENCES conversations(id) ON DELETE SET NULL,
    reviewed_by         UUID REFERENCES users(id) ON DELETE SET NULL,
    reviewed_at         TIMESTAMPTZ,
    applied_at          TIMESTAMPTZ,
    apply_error         TEXT,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_proposals_status ON change_proposals(status, created_at DESC);

ALTER TABLE tool_versions
    ADD CONSTRAINT tool_versions_proposal_fk
    FOREIGN KEY (proposal_id) REFERENCES change_proposals(id) ON DELETE SET NULL;

ALTER TABLE agent_versions
    ADD CONSTRAINT agent_versions_proposal_fk
    FOREIGN KEY (proposal_id) REFERENCES change_proposals(id) ON DELETE SET NULL;

-- ---------- Feedback (one row per user per run/tool execution) ----------

CREATE TABLE feedback (
    id                       UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id                  UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    scope                    TEXT NOT NULL CHECK (scope IN ('tool_run', 'agent_run')),
    conversation_id          UUID REFERENCES conversations(id) ON DELETE CASCADE,
    run_id                   UUID REFERENCES runs(id) ON DELETE SET NULL,
    tool_execution_id        UUID REFERENCES tool_executions(id) ON DELETE SET NULL,
    tool_id                  UUID REFERENCES tools(id) ON DELETE SET NULL,
    agent_id                 UUID REFERENCES agents(id) ON DELETE SET NULL,
    rating                   SMALLINT NOT NULL CHECK (rating IN (-1, 1)),
    comment                  TEXT NOT NULL DEFAULT '',
    status                   TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'addressed', 'dismissed')),
    addressed_by_proposal_id UUID REFERENCES change_proposals(id) ON DELETE SET NULL,
    -- stable upsert key: 'tool:{tool_call_id}' or 'agent:{run_id}'
    subject_key              TEXT NOT NULL,
    created_at               TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at               TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (user_id, subject_key)
);

CREATE INDEX idx_feedback_target ON feedback(tool_id, agent_id, status);

-- ---------- Files (attachments + artifacts) ----------

CREATE TABLE files (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    conversation_id UUID REFERENCES conversations(id) ON DELETE CASCADE,
    message_id      UUID REFERENCES messages(id) ON DELETE SET NULL,
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    origin          TEXT NOT NULL CHECK (origin IN ('attachment', 'artifact')),
    filename        TEXT NOT NULL CHECK (filename <> ''),
    content_type    TEXT NOT NULL DEFAULT 'application/octet-stream'
                    CHECK (content_type ~ '^[!-~]+$'),
    size_bytes      BIGINT NOT NULL CHECK (size_bytes >= 0),
    storage_key     TEXT NOT NULL UNIQUE CHECK (storage_key <> ''),
    checksum_sha256 TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_files_conversation ON files(conversation_id);
CREATE INDEX idx_files_user ON files(user_id);

-- ---------- Usage & audit ----------

CREATE TABLE usage_logs (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id           UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    conversation_id   UUID REFERENCES conversations(id) ON DELETE SET NULL,
    model_id          UUID REFERENCES models(id) ON DELETE SET NULL,
    prompt_tokens     INT,
    completion_tokens INT,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_usage_user ON usage_logs(user_id, created_at);

CREATE TABLE audit_logs (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id    UUID REFERENCES users(id) ON DELETE SET NULL,
    action     TEXT NOT NULL,
    target     TEXT NOT NULL DEFAULT '',
    allowed    BOOLEAN NOT NULL DEFAULT TRUE,
    detail     JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_audit_user ON audit_logs(user_id, created_at);
