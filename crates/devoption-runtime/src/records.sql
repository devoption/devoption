-- Operator migration, applied using a separate migration identity.
-- Use a dedicated application database; never platform owner/catalog storage.
CREATE SCHEMA app_records;
CREATE TABLE app_records.entries (
    app_id text NOT NULL,
    collection text NOT NULL,
    id text NOT NULL,
    payload jsonb NOT NULL CHECK (jsonb_typeof(payload) = 'object'),
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (app_id, collection, id)
);
-- Grant the runtime role USAGE on this schema and INSERT/SELECT on this table.
-- No UPDATE/DELETE/DDL grant is needed. The guest has no SELECT operation.
