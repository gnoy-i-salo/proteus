-- Schema v7: persistent research and frozen persona artifacts. Publication
-- validates DTOs, source membership and live grants before these atomic writes.
-- Payloads remain separate from original revisions and retrieval indexes.
CREATE TABLE memory_items (
    id TEXT PRIMARY KEY,
    corpus_snapshot_id TEXT NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
    persona_id TEXT NOT NULL,
    payload_bytes INTEGER NOT NULL CHECK(payload_bytes BETWEEN 1 AND 524288),
    data_json TEXT NOT NULL CHECK(length(CAST(data_json AS BLOB))=payload_bytes),
    canonical_sha256 TEXT NOT NULL CHECK(length(canonical_sha256)=64 AND canonical_sha256 NOT GLOB '*[^0-9a-f]*')
);
CREATE INDEX memory_items_scope ON memory_items(corpus_snapshot_id,persona_id,id);
CREATE TRIGGER memory_items_immutable BEFORE UPDATE ON memory_items
BEGIN SELECT RAISE(ABORT,'immutable memory item'); END;

CREATE TABLE memory_snapshots (
    id TEXT PRIMARY KEY,
    corpus_snapshot_id TEXT NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
    persona_id TEXT NOT NULL,
    payload_bytes INTEGER NOT NULL CHECK(payload_bytes BETWEEN 1 AND 1048576),
    data_json TEXT NOT NULL CHECK(length(CAST(data_json AS BLOB))=payload_bytes),
    canonical_sha256 TEXT NOT NULL CHECK(length(canonical_sha256)=64 AND canonical_sha256 NOT GLOB '*[^0-9a-f]*')
);
CREATE INDEX memory_snapshots_scope ON memory_snapshots(corpus_snapshot_id,persona_id,id);
CREATE TRIGGER memory_snapshots_immutable BEFORE UPDATE ON memory_snapshots
BEGIN SELECT RAISE(ABORT,'immutable memory snapshot'); END;

CREATE TABLE memory_snapshot_items (
    snapshot_id TEXT NOT NULL REFERENCES memory_snapshots(id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK(position>=0 AND position<1000),
    item_id TEXT NOT NULL REFERENCES memory_items(id) ON DELETE CASCADE,
    PRIMARY KEY(snapshot_id,position), UNIQUE(snapshot_id,item_id)
);
CREATE TRIGGER memory_snapshot_items_scope BEFORE INSERT ON memory_snapshot_items
WHEN NOT EXISTS(SELECT 1 FROM memory_snapshots s JOIN memory_items i
    ON i.corpus_snapshot_id=s.corpus_snapshot_id AND i.persona_id=s.persona_id
    WHERE s.id=NEW.snapshot_id AND i.id=NEW.item_id)
BEGIN SELECT RAISE(ABORT,'memory item outside snapshot scope'); END;
CREATE TRIGGER memory_snapshot_items_immutable BEFORE UPDATE ON memory_snapshot_items
BEGIN SELECT RAISE(ABORT,'immutable memory snapshot membership'); END;

CREATE TABLE memory_manifests (
    id TEXT PRIMARY KEY,
    corpus_snapshot_id TEXT NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
    memory_snapshot_id TEXT NOT NULL REFERENCES memory_snapshots(id) ON DELETE CASCADE,
    persona_id TEXT NOT NULL,
    payload_bytes INTEGER NOT NULL CHECK(payload_bytes BETWEEN 1 AND 8388608),
    data_json TEXT NOT NULL CHECK(length(CAST(data_json AS BLOB))=payload_bytes),
    canonical_sha256 TEXT NOT NULL CHECK(length(canonical_sha256)=64 AND canonical_sha256 NOT GLOB '*[^0-9a-f]*')
);
CREATE INDEX memory_manifests_scope ON memory_manifests(corpus_snapshot_id,persona_id,id);
CREATE TRIGGER memory_manifests_scope_guard BEFORE INSERT ON memory_manifests
WHEN NOT EXISTS(SELECT 1 FROM memory_snapshots s WHERE s.id=NEW.memory_snapshot_id
    AND s.corpus_snapshot_id=NEW.corpus_snapshot_id AND s.persona_id=NEW.persona_id)
BEGIN SELECT RAISE(ABORT,'manifest outside memory snapshot scope'); END;
CREATE TRIGGER memory_manifests_immutable BEFORE UPDATE ON memory_manifests
BEGIN SELECT RAISE(ABORT,'immutable persona manifest'); END;

CREATE TABLE memory_skills (
    id TEXT PRIMARY KEY,
    manifest_id TEXT NOT NULL REFERENCES memory_manifests(id) ON DELETE CASCADE,
    payload_bytes INTEGER NOT NULL CHECK(payload_bytes BETWEEN 1 AND 524288),
    data_json TEXT NOT NULL CHECK(length(CAST(data_json AS BLOB))=payload_bytes),
    canonical_sha256 TEXT NOT NULL CHECK(length(canonical_sha256)=64 AND canonical_sha256 NOT GLOB '*[^0-9a-f]*')
);
CREATE INDEX memory_skills_manifest ON memory_skills(manifest_id,id);
CREATE TRIGGER memory_skills_immutable BEFORE UPDATE ON memory_skills
BEGIN SELECT RAISE(ABORT,'immutable persona skill'); END;

CREATE TABLE memory_publications (
    id TEXT PRIMARY KEY,
    memory_snapshot_id TEXT NOT NULL UNIQUE REFERENCES memory_snapshots(id) ON DELETE CASCADE,
    input_sha256 TEXT NOT NULL CHECK(length(input_sha256)=64 AND input_sha256 NOT GLOB '*[^0-9a-f]*'),
    payload_bytes INTEGER NOT NULL CHECK(payload_bytes BETWEEN 1 AND 16777216),
    data_json TEXT NOT NULL CHECK(length(CAST(data_json AS BLOB))=payload_bytes),
    canonical_sha256 TEXT NOT NULL CHECK(length(canonical_sha256)=64 AND canonical_sha256 NOT GLOB '*[^0-9a-f]*')
);
CREATE TRIGGER memory_publications_immutable BEFORE UPDATE ON memory_publications
BEGIN SELECT RAISE(ABORT,'immutable research publication receipt'); END;
