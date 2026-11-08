-- Schema v6: derived vectors in the same encrypted corpus database.
-- A staging generation owns its blobs; publication changes only its sealed
-- metadata and the active pointer. No second full vector copy is needed.
CREATE TABLE vector_generations (
    generation INTEGER PRIMARY KEY AUTOINCREMENT CHECK(generation>0 AND generation<=9007199254740991),
    stage_id TEXT NOT NULL UNIQUE,
    snapshot_id TEXT NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
    snapshot_generation INTEGER NOT NULL CHECK(snapshot_generation>0),
    search_generation INTEGER NOT NULL CHECK(search_generation>0),
    source_receipt_json TEXT NOT NULL,
    model_fingerprint TEXT NOT NULL,
    dimensions INTEGER NOT NULL CHECK(dimensions BETWEEN 1 AND 4096),
    state TEXT NOT NULL DEFAULT 'staging' CHECK(state IN ('staging','published')),
    vector_count INTEGER NOT NULL DEFAULT 0 CHECK(vector_count BETWEEN 0 AND 1000000),
    content_sha256 TEXT,
    created_at TEXT NOT NULL,
    published_at TEXT,
    UNIQUE(snapshot_id,generation),
    FOREIGN KEY(snapshot_id,search_generation) REFERENCES search_generations(snapshot_id,index_generation) ON DELETE CASCADE,
    CHECK((state='staging' AND content_sha256 IS NULL AND published_at IS NULL) OR
          (state='published' AND content_sha256 IS NOT NULL AND length(content_sha256)=64 AND published_at IS NOT NULL))
);
CREATE TRIGGER vector_generations_immutable BEFORE UPDATE ON vector_generations
WHEN OLD.state='published' OR NEW.generation<>OLD.generation OR NEW.stage_id<>OLD.stage_id
 OR NEW.snapshot_id<>OLD.snapshot_id OR NEW.snapshot_generation<>OLD.snapshot_generation
 OR NEW.search_generation<>OLD.search_generation OR NEW.source_receipt_json<>OLD.source_receipt_json
 OR NEW.model_fingerprint<>OLD.model_fingerprint OR NEW.dimensions<>OLD.dimensions
 OR NEW.created_at<>OLD.created_at
BEGIN SELECT RAISE(ABORT,'immutable vector generation'); END;

CREATE TABLE vector_entries (
    generation INTEGER NOT NULL REFERENCES vector_generations(generation) ON DELETE CASCADE,
    chunk_id TEXT NOT NULL,
    vector BLOB NOT NULL CHECK(typeof(vector)='blob' AND length(vector)>0 AND length(vector)<=16384 AND length(vector)%4=0),
    PRIMARY KEY(generation,chunk_id)
);
CREATE TRIGGER vector_entries_immutable BEFORE UPDATE ON vector_entries
BEGIN SELECT RAISE(ABORT,'immutable vector entry'); END;
CREATE TRIGGER vector_entries_staging_insert BEFORE INSERT ON vector_entries
WHEN NOT EXISTS(SELECT 1 FROM vector_generations g WHERE g.generation=NEW.generation
                AND g.state='staging' AND length(NEW.vector)=4*g.dimensions)
BEGIN SELECT RAISE(ABORT,'vector entry requires matching staging generation'); END;

CREATE TABLE vector_active (
    snapshot_id TEXT PRIMARY KEY REFERENCES snapshots(id) ON DELETE CASCADE,
    generation INTEGER NOT NULL,
    FOREIGN KEY(snapshot_id,generation) REFERENCES vector_generations(snapshot_id,generation) ON DELETE CASCADE
);
CREATE TRIGGER vector_active_published_insert BEFORE INSERT ON vector_active
WHEN NOT EXISTS(SELECT 1 FROM vector_generations g WHERE g.snapshot_id=NEW.snapshot_id
                AND g.generation=NEW.generation AND g.state='published')
BEGIN SELECT RAISE(ABORT,'active vector generation must be published'); END;
CREATE TRIGGER vector_active_published_update BEFORE UPDATE ON vector_active
WHEN NOT EXISTS(SELECT 1 FROM vector_generations g WHERE g.snapshot_id=NEW.snapshot_id
                AND g.generation=NEW.generation AND g.state='published')
BEGIN SELECT RAISE(ABORT,'active vector generation must be published'); END;
