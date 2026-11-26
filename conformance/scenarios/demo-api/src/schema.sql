-- The demo API's database (conformance/demo/README.md, "The transitions database"): every transition of the
-- network the demo runs on, as the adapters (the providers) and the outer pieces they write to (banks' and
-- ledgers' books, chains) recorded it, through the API. It is the demo's world, not Core's log or state: Core's own
-- records are its log's, checkable with audit-check.
--
-- The server applies this at start, idempotently. Version 1.

CREATE TABLE IF NOT EXISTS demo_schema (
    version    INTEGER PRIMARY KEY,
    applied_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- One run of the network: one Core put together, from a seed, until the server stops or Core is reset.
CREATE TABLE IF NOT EXISTS demo_runs (
    run        TEXT PRIMARY KEY,
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    seed       BIGINT NOT NULL,
    -- The network the run started on: its nodes and edges, as GET /network gives them.
    network    JSONB NOT NULL
);

-- One transition: a request an adapter made of an outer piece, and how it came out.
CREATE TABLE IF NOT EXISTS demo_transitions (
    run            TEXT NOT NULL REFERENCES demo_runs (run),
    seq            BIGINT NOT NULL,
    recorded_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Core's clock when the API recorded it, RFC 3339: simulated time, in the demo.
    world_at       TEXT,
    -- Who sent it to the API: `simulator`, or the name an adapter gives itself.
    source         TEXT NOT NULL,
    -- The outer piece it reached: `book` (a provider's book), `chain` (a public network), `ledger` (a publisher's
    -- private network), `rail` (an outer network: a bank's rail, a chain's addresses), or
    -- what an adapter names.
    piece          TEXT NOT NULL,
    -- What it was: withdrawal, deposit, transfer, conversion, reversal, …, or `movement` on a chain.
    kind           TEXT NOT NULL,
    provider       TEXT,
    network        TEXT,
    order_id       TEXT,
    segment_id     TEXT,
    transaction_id TEXT,
    from_node      TEXT,
    to_node        TEXT,
    asset_in       TEXT,
    asset_out      TEXT,
    amount_in      TEXT,
    amount_out     TEXT,
    dry_run        BOOLEAN NOT NULL DEFAULT FALSE,
    -- How it came out: answered, failed, refused, unknown, … for an operation; recorded or refused for a movement.
    outcome        TEXT NOT NULL,
    -- The request as the piece received it.
    detail         JSONB NOT NULL,
    PRIMARY KEY (run, seq)
);

CREATE INDEX IF NOT EXISTS demo_transitions_order ON demo_transitions (order_id);
CREATE INDEX IF NOT EXISTS demo_transitions_provider ON demo_transitions (provider);

INSERT INTO demo_schema (version) VALUES (1) ON CONFLICT DO NOTHING;
