-- How long a feed's server has to answer, and who the fetch says it is
ALTER TABLE feed ADD COLUMN timeout_seconds INTEGER NOT NULL DEFAULT 20
    CHECK (timeout_seconds BETWEEN 1 AND 600);
ALTER TABLE feed ADD COLUMN user_agent TEXT;
