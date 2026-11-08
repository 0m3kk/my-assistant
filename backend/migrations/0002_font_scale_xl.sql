ALTER TABLE user_preferences DROP CONSTRAINT IF EXISTS user_preferences_font_scale_check;

ALTER TABLE user_preferences
    ADD CONSTRAINT user_preferences_font_scale_check
    CHECK (font_scale IN ('sm', 'md', 'lg', 'xl'));
