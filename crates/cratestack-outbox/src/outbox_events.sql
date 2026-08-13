CREATE TABLE IF NOT EXISTS cratestack_outbox_events (
    id TEXT PRIMARY KEY,
    aggregate_type TEXT NOT NULL,
    aggregate_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    payload JSONB NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL,
    correlation_id TEXT
);

-- The drain cursor sorts on `id` (UUIDv7 -- timestamp-prefixed and
-- lexically monotonic), and the GC sweep filters on occurred_at. Both
-- get their own index so neither degrades to a sequential scan.
CREATE INDEX IF NOT EXISTS idx_cratestack_outbox_events_id
    ON cratestack_outbox_events (id);

CREATE INDEX IF NOT EXISTS idx_cratestack_outbox_events_occurred_at
    ON cratestack_outbox_events (occurred_at);
