-- Admin-defined, one-level folders for tools and agents.
--
-- Tools and agents have separate folder sets: a tool can only be assigned to
-- a tool folder and an agent only to an agent folder. Deleting a folder moves
-- its items back to "Ungrouped" (ON DELETE SET NULL).

CREATE TABLE IF NOT EXISTS tool_groups (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name       TEXT NOT NULL,
    position   INT  NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_tool_groups_name ON tool_groups (lower(name));

CREATE TABLE IF NOT EXISTS agent_groups (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name       TEXT NOT NULL,
    position   INT  NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_groups_name ON agent_groups (lower(name));

ALTER TABLE tools
    ADD COLUMN IF NOT EXISTS group_id UUID REFERENCES tool_groups(id) ON DELETE SET NULL;
ALTER TABLE agents
    ADD COLUMN IF NOT EXISTS group_id UUID REFERENCES agent_groups(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS idx_tools_group ON tools(group_id);
CREATE INDEX IF NOT EXISTS idx_agents_group ON agents(group_id);
