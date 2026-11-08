-- Durable history of generated simulation dialogue. This namespace is physically
-- separate from source documents/revisions, snapshots, MemoryItems and vector/FTS
-- records: generated text never enters original corpus tables. Rows reference the
-- root-owned manifest registry and are removed with their manifest.
CREATE TABLE simulation_entries (
    id TEXT PRIMARY KEY,
    experiment_id TEXT NOT NULL,
    manifest_id TEXT NOT NULL REFERENCES memory_manifests(id) ON DELETE CASCADE,
    trial_id TEXT NOT NULL,
    sequence INTEGER NOT NULL CHECK(sequence > 0),
    payload_bytes INTEGER NOT NULL CHECK(payload_bytes > 0),
    data_json TEXT NOT NULL,
    canonical_sha256 TEXT NOT NULL,
    UNIQUE(manifest_id, experiment_id, trial_id, sequence)
);
CREATE INDEX simulation_entries_scope ON simulation_entries(manifest_id, experiment_id, trial_id, sequence);
