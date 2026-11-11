-- How long a feed's server has to answer, and the user agent its fetches send
ALTER TABLE feed ADD COLUMN timeout_seconds INTEGER NOT NULL DEFAULT 20
    CHECK (timeout_seconds BETWEEN 1 AND 600);
ALTER TABLE feed ADD COLUMN user_agent TEXT;

-- The daemon asks for feeds by their next refresh
CREATE INDEX feed_by_next_refresh ON feed (next_refresh_at) WHERE disabled = 0;
