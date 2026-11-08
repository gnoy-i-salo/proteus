//! Storage integration uses real SQLCipher/schema7, while manifest publication
//! authority remains a separate host concern. The small manifest rows below are
//! deliberate SQL fixtures, not a production manifest writer.
use super::*;
use crate::corpus_database::{Acquisition, RecordInput, RevisionInput, SourceProvenance};
use std::{cell::Cell, path::PathBuf};

const AT: &str = "2026-11-09T00:00:00+03:00";
const KEY: [u8; 32] = [73; 32];
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "proteus-simulation-encrypted-{}",
            rand::random::<u64>()
        )))
    }
    fn open(&self) -> CorpusDatabase {
        CorpusDatabase::open(&self.0.join("corpus.db"), &KEY).unwrap()
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn seed(db: &mut CorpusDatabase, owner: &str) -> String {
    let generation = db
        .save_ingestion(&RecordInput {
            id: owner.into(),
            kind: "import".into(),
            provider: "telegram".into(),
            state_json: "{}".into(),
            documents: vec![RevisionInput {
                source_key: format!("chat/{owner}"),
                document_id: owner.into(),
                document_json: "{\"text\":\"  Исходник é\\r\\n😀  \"}".into(),
                provenance: SourceProvenance::source_service(Acquisition::Archive),
            }],
        })
        .unwrap();
    let snapshot = db.publish(owner, owner, &[generation]).unwrap();
    db.connection.execute("INSERT INTO memory_snapshots(id,corpus_snapshot_id,persona_id,payload_bytes,data_json,canonical_sha256) VALUES(?1,?2,?1,2,'{}',?3)",params![owner,snapshot.id,"a".repeat(64)]).unwrap();
    db.connection.execute("INSERT INTO memory_manifests(id,corpus_snapshot_id,memory_snapshot_id,persona_id,payload_bytes,data_json,canonical_sha256) VALUES(?1,?2,?1,?1,2,'{}',?3)",params![owner,snapshot.id,"a".repeat(64)]).unwrap();
    snapshot.id
}
fn draft(owner: &str) -> SimulationMemoryDraft {
    SimulationMemoryDraft {
        schema_version: 1,
        experiment_id: "experiment".into(),
        manifest_id: owner.into(),
        trial_id: "trial".into(),
        role: SimulationRole::Simulation,
        text: "  Симуляция é\r\n😀 <text>  ".into(),
    }
}
fn query(owner: &str) -> SimulationPageQuery<'_> {
    SimulationPageQuery {
        manifest_id: owner,
        experiment_id: "experiment",
        trial_id: "trial",
        after_sequence: 0,
        limit: 10,
        max_bytes: 4096,
    }
}
#[test]
fn encrypted_history_reopens_replays_and_preserves_original_author_bytes() {
    let dir = Directory::new();
    let mut db = dir.open();
    let snapshot = seed(&mut db, "a");
    let original = db.read("a").unwrap().unwrap();
    let frozen = db.snapshot(&snapshot).unwrap();
    let entry = db
        .append_simulation_checked(&draft("a"), "reply", AT, &|| Ok(()))
        .unwrap();
    drop(db);
    let mut db = dir.open();
    assert_eq!(
        db.simulation_page_checked(&query("a"), &|| Ok(()))
            .unwrap()
            .items[0]
            .entry,
        entry
    );
    assert_eq!(
        db.append_simulation_checked(&draft("a"), "reply", AT, &|| Ok(()))
            .unwrap(),
        entry
    );
    assert_eq!(db.read("a").unwrap().unwrap(), original);
    assert_eq!(db.snapshot(&snapshot).unwrap(), frozen);
    assert_eq!(
        db.simulation_page_checked(&query("a"), &|| Err("revoked exactly".into()))
            .unwrap_err(),
        "revoked exactly"
    );
    drop(db);
    assert!(CorpusDatabase::open(&dir.0.join("corpus.db"), &[74; 32]).is_err());
    let ciphertext = std::fs::read(dir.0.join("corpus.db")).unwrap();
    for word in ["Симуляция", "Исходник"] {
        assert!(!ciphertext
            .windows(word.len())
            .any(|bytes| bytes == word.as_bytes()));
    }
}
#[test]
fn source_revocation_cascades_only_its_frozen_simulation_history() {
    let dir = Directory::new();
    let mut db = dir.open();
    seed(&mut db, "a");
    seed(&mut db, "b");
    db.append_simulation(&draft("a"), "reply-a", AT).unwrap();
    let b = db.append_simulation(&draft("b"), "reply-b", AT).unwrap();
    let original_b = db.read("b").unwrap().unwrap();
    db.remove("a").unwrap();
    assert!(db.simulation_page(&query("a")).unwrap().items.is_empty());
    assert!(db.append_simulation(&draft("a"), "late-a", AT).is_err());
    assert_eq!(db.simulation_page(&query("b")).unwrap().items[0].entry, b);
    assert_eq!(db.read("b").unwrap().unwrap(), original_b);
    assert_eq!(
        db.connection
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r
                .get::<_, i64>(
                0
            ))
            .unwrap(),
        0
    );
}
#[test]
fn encrypted_append_rolls_back_at_every_authorization_checkpoint() {
    let dir = Directory::new();
    let mut db = dir.open();
    seed(&mut db, "a");
    let calls = Cell::new(0);
    db.append_simulation_checked(&draft("a"), "baseline", AT, &|| {
        calls.set(calls.get() + 1);
        Ok(())
    })
    .unwrap();
    for fail_at in 1..=calls.get() {
        let current = Cell::new(0);
        let id = format!("cancel-{fail_at}");
        assert_eq!(
            db.append_simulation_checked(&draft("a"), &id, AT, &|| {
                current.set(current.get() + 1);
                if current.get() == fail_at {
                    Err("  cancelled é  ".into())
                } else {
                    Ok(())
                }
            })
            .unwrap_err(),
            "  cancelled é  "
        );
        assert_eq!(db.simulation_page(&query("a")).unwrap().items.len(), 1);
    }
    drop(db);
    assert_eq!(
        dir.open().simulation_page(&query("a")).unwrap().items.len(),
        1
    );
}
