-- One-level user-defined conversation folders.
--
-- A conversation belongs to at most one folder. Deleting a folder ungroups
-- its conversations instead of deleting them (ON DELETE SET NULL).

CREATE TABLE IF NOT EXISTS conversation_groups (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    position   INT  NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_conversation_groups_user_name
    ON conversation_groups (user_id, lower(name));
CREATE INDEX IF NOT EXISTS idx_conversation_groups_user
    ON conversation_groups (user_id, position);

ALTER TABLE conversations
    ADD COLUMN IF NOT EXISTS group_id UUID REFERENCES conversation_groups(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS idx_conversations_group
    ON conversations (group_id, last_message_at DESC);
