-- Background jobs, polled by the in-process job loop with FOR UPDATE SKIP LOCKED.
-- `kind` is free text (an open enum): unknown kinds are retried, not dropped, so a rolling
-- deploy can enqueue kinds that only newer instances understand.

CREATE TABLE jobs (
    id           uuid PRIMARY KEY,
    kind         text NOT NULL,
    payload      jsonb NOT NULL DEFAULT '{}',
    run_at       timestamptz NOT NULL DEFAULT now(),
    -- Lease: set when claimed; a stale lease (crashed worker) can be reclaimed.
    locked_at    timestamptz,
    locked_by    text,
    attempts     int NOT NULL DEFAULT 0,
    max_attempts int NOT NULL DEFAULT 10,
    last_error   text,
    completed_at timestamptz,
    failed_at    timestamptz,
    -- At most one *waiting* (unclaimed) job per key, e.g. one auto-confirm per match. A
    -- running job may enqueue its own successor under the same key.
    dedupe_key   text,
    created_at   timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX jobs_due_idx ON jobs (run_at) WHERE completed_at IS NULL AND failed_at IS NULL;
CREATE UNIQUE INDEX jobs_waiting_dedupe_key ON jobs (dedupe_key)
    WHERE completed_at IS NULL AND failed_at IS NULL AND locked_at IS NULL
      AND dedupe_key IS NOT NULL;

GRANT SELECT, INSERT, UPDATE, DELETE ON jobs TO courtpit_app;
