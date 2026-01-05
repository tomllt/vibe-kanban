-- Lightweight derived state from webhook events (push/check_run) for observability
CREATE TABLE IF NOT EXISTS webhook_repo_states (
    id                      BLOB PRIMARY KEY,
    provider                TEXT NOT NULL CHECK (provider IN ('github', 'gitlab')),
    repo_key                TEXT NOT NULL,

    last_push_ref           TEXT,
    last_push_sha           TEXT,

    last_check_run_sha      TEXT,
    last_check_run_status   TEXT,
    last_check_run_conclusion TEXT,

    updated_at              TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    UNIQUE (provider, repo_key)
);

CREATE INDEX IF NOT EXISTS idx_webhook_repo_states_provider
    ON webhook_repo_states(provider);
CREATE INDEX IF NOT EXISTS idx_webhook_repo_states_repo_key
    ON webhook_repo_states(repo_key);

