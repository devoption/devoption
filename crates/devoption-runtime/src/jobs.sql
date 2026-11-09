-- Apply only with an operator migration identity to a dedicated app database.
CREATE SCHEMA app_jobs;
CREATE TABLE app_jobs.deliveries (
    app_id text NOT NULL CHECK (app_id ~ '^[0-9a-f]{32}$'),
    id text NOT NULL CHECK (id ~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'),
    policy text NOT NULL CHECK (policy ~ '^[0-9a-f]{64}$'),
    payload jsonb NOT NULL CHECK (jsonb_typeof(payload)='object' AND octet_length(payload::text)<=65536),
    delay_seconds integer NOT NULL CHECK (delay_seconds BETWEEN 0 AND 3600),
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    due_at timestamptz NOT NULL,
    state text NOT NULL CHECK (state IN ('queued','leased','retry_wait','succeeded','failed','cancelled','needs_review')),
    attempts integer NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 5),
    generation bigint NOT NULL DEFAULT 0,
    lease_until timestamptz,
    outcome text NOT NULL DEFAULT 'pending' CHECK (length(outcome)<=32),
    PRIMARY KEY (app_id,id)
);
-- Enqueue role: schema USAGE and table INSERT/SELECT only.
-- Separate dispatcher/control role: SELECT and UPDATE of state, attempts,
-- generation, lease_until, due_at and outcome only; never payload/identity,
-- INSERT, DDL or DELETE.
-- No automatic retention/deletion: deduplication identities must survive retries
-- and supported backup restore. Capacity refusal requires operator resolution.
