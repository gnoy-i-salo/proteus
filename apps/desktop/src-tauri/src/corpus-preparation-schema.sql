-- Schema v5: encrypted immutable derived snapshots. Deleting/revoking their
-- original snapshots/records also removes episode text and preparation metadata.
CREATE TABLE corpus_preparations (
    snapshot_id TEXT PRIMARY KEY REFERENCES snapshots(id) ON DELETE CASCADE,
    source_snapshot_id TEXT NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
    split_version TEXT NOT NULL, tokenizer_version TEXT NOT NULL, chunking_version TEXT NOT NULL,
    canonical_sha256 TEXT NOT NULL
);
CREATE TRIGGER immutable_corpus_preparations BEFORE UPDATE ON corpus_preparations BEGIN SELECT RAISE(ABORT,'immutable preparation'); END;
CREATE TABLE corpus_episodes (
    snapshot_id TEXT NOT NULL REFERENCES corpus_preparations(snapshot_id) ON DELETE CASCADE,
    episode_id TEXT NOT NULL, position INTEGER NOT NULL CHECK(position>=0),
    data_json TEXT NOT NULL, canonical_sha256 TEXT NOT NULL,
    PRIMARY KEY(snapshot_id,episode_id), UNIQUE(snapshot_id,position)
);
CREATE TRIGGER immutable_corpus_episodes BEFORE UPDATE ON corpus_episodes BEGIN SELECT RAISE(ABORT,'immutable episode'); END;
CREATE TABLE corpus_prepared_chunks (
    snapshot_id TEXT NOT NULL, chunk_id TEXT NOT NULL, episode_id TEXT NOT NULL,
    position INTEGER NOT NULL CHECK(position>=0), data_json TEXT NOT NULL, canonical_sha256 TEXT NOT NULL,
    PRIMARY KEY(snapshot_id,chunk_id), UNIQUE(snapshot_id,position),
    FOREIGN KEY(snapshot_id,episode_id) REFERENCES corpus_episodes(snapshot_id,episode_id) ON DELETE CASCADE
);
CREATE INDEX corpus_prepared_chunks_episode ON corpus_prepared_chunks(snapshot_id,episode_id,position);
CREATE TRIGGER immutable_corpus_prepared_chunks BEFORE UPDATE ON corpus_prepared_chunks BEGIN SELECT RAISE(ABORT,'immutable prepared chunk'); END;
