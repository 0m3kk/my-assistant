-- LLM-generated conversation titles.
--
-- `user_preferences.title_model_id` selects the model used to generate a
-- conversation title. NULL means "use the global default model".
--
-- `conversations.title_generated` is FALSE while the title is still the
-- first-message heuristic and TRUE once it has been generated (or set
-- manually). Background generation only runs while it is FALSE, which keeps
-- manual renames from being overwritten.

ALTER TABLE user_preferences
    ADD COLUMN IF NOT EXISTS title_model_id UUID REFERENCES models(id) ON DELETE SET NULL;

ALTER TABLE conversations
    ADD COLUMN IF NOT EXISTS title_generated BOOLEAN NOT NULL DEFAULT FALSE;

-- Existing conversations keep their current titles; only new ones are
-- eligible for automatic LLM titling.
UPDATE conversations SET title_generated = TRUE;
