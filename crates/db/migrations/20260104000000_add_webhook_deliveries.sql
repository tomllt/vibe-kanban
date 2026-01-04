-- Track incoming webhook deliveries for idempotency + observability
CREATE TABLE IF NOT EXISTS webhook_deliveries (
    id              BLOB PRIMARY KEY,
    provider        TEXT NOT NULL CHECK (provider IN ('github', 'gitlab')),
    delivery_id     TEXT NOT NULL,
    event           TEXT NOT NULL,
    signature_valid INTEGER NOT NULL DEFAULT 0,
    received_at     TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    processed_at    TEXT,
    status          TEXT NOT NULL DEFAULT 'received'
                       CHECK (status IN ('received', 'processed', 'ignored', 'failed')),
    attempts        INTEGER NOT NULL DEFAULT 0,
    last_error      TEXT,
    payload_json    TEXT,
    UNIQUE (provider, delivery_id)
);

CREATE INDEX IF NOT EXISTS idx_webhook_deliveries_provider_event
    ON webhook_deliveries(provider, event);
CREATE INDEX IF NOT EXISTS idx_webhook_deliveries_status
    ON webhook_deliveries(status);

