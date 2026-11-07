-- Frozen schema v4, before prepared snapshot storage.
CREATE TABLE records (
    id TEXT PRIMARY KEY, kind TEXT NOT NULL CHECK(kind IN ('grant','import','deleted')),
    provider TEXT NOT NULL, state_json TEXT NOT NULL, canonical_sha256 TEXT NOT NULL,
    generation INTEGER NOT NULL CHECK(generation>0),
    namespace TEXT NOT NULL DEFAULT 'sourceService' CHECK(namespace IN ('sourceService','ingestion'))
);
CREATE TRIGGER immutable_record_namespace BEFORE UPDATE OF namespace ON records WHEN NEW.namespace<>OLD.namespace BEGIN SELECT RAISE(ABORT,'immutable record namespace'); END;
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
    position INTEGER NOT NULL CHECK(position>=0),record_generation INTEGER NOT NULL CHECK(record_generation>0),
    PRIMARY KEY(job_id,position),UNIQUE(job_id,record_id)
);
PRAGMA application_id=1347572819;
PRAGMA user_version=4;

-- Schema v4: native corpus FTS5/BM25 search index.
-- The corpus owner applies this file inside the next explicit, versioned corpus
-- migration together with PRAGMA user_version=4. The search
-- module never creates or alters these objects at runtime: opening an unmigrated
-- database fails closed instead of lazy-migrating.
-- All objects live in the same SQLCipher database as the corpus; there are no
-- plaintext index files. Chunk text is stored only inside FTS5 tables, metadata
-- only in ordinary tables, joined by an explicit integer rowid.

CREATE TABLE search_stages (
    stage_id TEXT PRIMARY KEY,
    snapshot_id TEXT NOT NULL,
    snapshot_generation INTEGER NOT NULL CHECK(snapshot_generation>0),
    policy_fingerprint TEXT NOT NULL,
    content_sha256 TEXT NOT NULL,
    chunk_count INTEGER NOT NULL DEFAULT 0 CHECK(chunk_count>=0),
    state TEXT NOT NULL DEFAULT 'staging' CHECK(state IN ('staging','published')),
    published_generation INTEGER CHECK(published_generation IS NULL OR published_generation>0),
    created_at TEXT NOT NULL
);

CREATE TABLE search_stage_chunks (
    stage_id TEXT NOT NULL,
    chunk_id TEXT NOT NULL,
    content_sha256 TEXT NOT NULL,
    snapshot_id TEXT NOT NULL,
    snapshot_generation INTEGER NOT NULL CHECK(snapshot_generation>0),
    partition TEXT NOT NULL,
    document_id TEXT NOT NULL,
    revision_id TEXT NOT NULL,
    span_start INTEGER NOT NULL CHECK(span_start>=0),
    span_end INTEGER NOT NULL CHECK(span_end>span_start),
    persona_id TEXT NOT NULL,
    grant_id TEXT NOT NULL,
    binding_id TEXT NOT NULL,
    record_id TEXT NOT NULL,
    record_generation INTEGER NOT NULL CHECK(record_generation>0),
    period_from INTEGER,
    period_to INTEGER,
    conversation_id TEXT,
    thread_id TEXT,
    PRIMARY KEY(stage_id,chunk_id)
);

CREATE VIRTUAL TABLE search_stage_fts USING fts5(text);

CREATE TABLE search_generations (
    snapshot_id TEXT NOT NULL,
    index_generation INTEGER NOT NULL CHECK(index_generation>0),
    snapshot_generation INTEGER NOT NULL CHECK(snapshot_generation>0),
    policy_fingerprint TEXT NOT NULL,
    content_sha256 TEXT NOT NULL,
    chunk_count INTEGER NOT NULL CHECK(chunk_count>=0),
    created_at TEXT NOT NULL,
    PRIMARY KEY(snapshot_id,index_generation)
);
CREATE TRIGGER immutable_search_generations BEFORE UPDATE ON search_generations BEGIN SELECT RAISE(ABORT,'immutable search generation'); END;

CREATE TABLE search_active (
    snapshot_id TEXT PRIMARY KEY,
    index_generation INTEGER NOT NULL CHECK(index_generation>0)
);

CREATE TABLE search_chunks (
    rowid INTEGER PRIMARY KEY,
    snapshot_id TEXT NOT NULL,
    index_generation INTEGER NOT NULL CHECK(index_generation>0),
    chunk_id TEXT NOT NULL,
    document_id TEXT NOT NULL,
    revision_id TEXT NOT NULL,
    span_start INTEGER NOT NULL CHECK(span_start>=0),
    span_end INTEGER NOT NULL CHECK(span_end>span_start),
    persona_id TEXT NOT NULL,
    grant_id TEXT NOT NULL,
    binding_id TEXT NOT NULL,
    record_id TEXT NOT NULL,
    record_generation INTEGER NOT NULL CHECK(record_generation>0),
    period_from INTEGER,
    period_to INTEGER,
    conversation_id TEXT,
    thread_id TEXT,
    UNIQUE(snapshot_id,index_generation,chunk_id)
);
CREATE TRIGGER immutable_search_chunks BEFORE UPDATE ON search_chunks BEGIN SELECT RAISE(ABORT,'immutable search chunk'); END;
CREATE INDEX search_chunks_scope ON search_chunks(snapshot_id,index_generation,persona_id,grant_id,binding_id);

CREATE VIRTUAL TABLE search_fts USING fts5(text);
