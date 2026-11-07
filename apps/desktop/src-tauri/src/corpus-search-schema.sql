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
