CREATE TABLE records (
    id TEXT PRIMARY KEY, kind TEXT NOT NULL CHECK(kind IN ('grant','import','deleted')),
    provider TEXT NOT NULL, state_json TEXT NOT NULL, canonical_sha256 TEXT NOT NULL,
    generation INTEGER NOT NULL CHECK(generation>0)
);
CREATE TABLE revisions (
    id TEXT PRIMARY KEY, source_key TEXT NOT NULL, document_id TEXT NOT NULL,
    document_json TEXT NOT NULL, provenance_json TEXT NOT NULL, canonical_sha256 TEXT NOT NULL,
    UNIQUE(source_key,canonical_sha256)
);
CREATE TRIGGER immutable_revisions BEFORE UPDATE ON revisions BEGIN SELECT RAISE(ABORT,'immutable revision'); END;
CREATE TABLE record_revisions (
    record_id TEXT NOT NULL REFERENCES records(id), position INTEGER NOT NULL CHECK(position>=0),
    revision_id TEXT NOT NULL REFERENCES revisions(id), PRIMARY KEY(record_id,position), UNIQUE(record_id,revision_id)
);
CREATE TABLE record_revision_history (
    record_id TEXT NOT NULL REFERENCES records(id),revision_id TEXT NOT NULL REFERENCES revisions(id),
    PRIMARY KEY(record_id,revision_id)
);
CREATE TABLE corpora (
    id TEXT PRIMARY KEY,last_generation INTEGER NOT NULL CHECK(last_generation>0),last_snapshot_id TEXT NOT NULL
);
CREATE TABLE publication_receipts (
    corpus_id TEXT NOT NULL,publication_id TEXT NOT NULL,snapshot_id TEXT NOT NULL UNIQUE,
    PRIMARY KEY(corpus_id,publication_id)
);
CREATE TRIGGER immutable_publication_receipts BEFORE UPDATE ON publication_receipts BEGIN SELECT RAISE(ABORT,'immutable publication receipt'); END;
CREATE TABLE snapshots (
    id TEXT PRIMARY KEY,corpus_id TEXT NOT NULL,publication_id TEXT NOT NULL,
    generation INTEGER NOT NULL CHECK(generation>0),parent_id TEXT,created_at TEXT NOT NULL,canonical_sha256 TEXT NOT NULL,
    UNIQUE(corpus_id,generation),UNIQUE(corpus_id,publication_id)
);
CREATE TRIGGER immutable_snapshots BEFORE UPDATE ON snapshots BEGIN SELECT RAISE(ABORT,'immutable snapshot'); END;
CREATE TABLE snapshot_records (
    snapshot_id TEXT NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,position INTEGER NOT NULL,
    record_id TEXT NOT NULL REFERENCES records(id),record_generation INTEGER NOT NULL,
    PRIMARY KEY(snapshot_id,position),UNIQUE(snapshot_id,record_id)
);
CREATE TABLE snapshot_revisions (
    snapshot_id TEXT NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,position INTEGER NOT NULL,
    revision_id TEXT NOT NULL REFERENCES revisions(id),PRIMARY KEY(snapshot_id,position),UNIQUE(snapshot_id,revision_id)
);
CREATE TABLE migration_audits (
    record_id TEXT PRIMARY KEY REFERENCES records(id),source_generation INTEGER NOT NULL,
    ciphertext_sha256 TEXT NOT NULL,canonical_sha256 TEXT NOT NULL,document_count INTEGER NOT NULL CHECK(document_count>=0),
    migrated_at TEXT NOT NULL
);
CREATE TABLE jobs (
    id TEXT PRIMARY KEY,schema_version INTEGER NOT NULL CHECK(schema_version=1),
    status TEXT NOT NULL CHECK(status IN ('queued','running','paused','cancelled','failed','completed')),
    phase TEXT NOT NULL CHECK(phase IN ('reading','normalizing','episodes','indexing','publishing')),
    state_json TEXT NOT NULL,canonical_sha256 TEXT NOT NULL
);
CREATE TABLE job_records (
    job_id TEXT NOT NULL REFERENCES jobs(id),record_id TEXT NOT NULL REFERENCES records(id),
    position INTEGER NOT NULL CHECK(position>=0),PRIMARY KEY(job_id,position),UNIQUE(job_id,record_id)
);
PRAGMA application_id=1347572819;
PRAGMA user_version=1;
