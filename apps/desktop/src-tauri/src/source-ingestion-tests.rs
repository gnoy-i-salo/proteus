use super::*;
use crate::corpus_database::{JobProvider, RecordInput};
fn modern(id: &str, provider: &str) -> RecordInput {
    RecordInput {
        id: id.into(),
        kind: "import".into(),
        provider: provider.into(),
        state_json: "{\"exact\":\"  é\\r\\n\"}".into(),
        documents: vec![],
    }
}
#[test]
fn legacy_clear_revokes_modern_records_in_the_same_database_but_keeps_other_providers() {
    let (sources, dir) = local();
    let tg = sources
        .begin_ingestion_operation("tg", JobProvider::Telegram)
        .unwrap();
    let github = sources
        .begin_ingestion_operation("github", JobProvider::Github)
        .unwrap();
    sources
        .with_ingestion_database(&tg, |db| {
            db.save_ingestion(&modern("tg-record", "telegram"))?;
            db.save_ingestion(&modern("github-record", "github"))?;
            Ok(())
        })
        .unwrap();
    sources.clear(Provider::Telegram).unwrap();
    sources
        .with_ingestion_database(&github, |db| {
            assert_eq!(db.read("tg-record")?.unwrap().input.kind, "deleted");
            assert_eq!(
                db.read("github-record")?.unwrap().input,
                modern("github-record", "github")
            );
            Ok(())
        })
        .unwrap();
    drop(tg);
    drop(github);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn a_read_finishing_after_provider_clear_cannot_restore_deleted_data() {
    let (sources, dir) = local();
    let operation = sources
        .begin_ingestion_operation("slow-read", JobProvider::Telegram)
        .unwrap();
    let copy = sources.clone();
    let (finish, resumed) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        resumed.recv().unwrap();
        copy.with_ingestion_database(&operation, |db| {
            db.save_ingestion(&modern("late", "telegram"))
        })
    });
    sources.clear(Provider::Telegram).unwrap();
    finish.send(()).unwrap();
    assert!(worker.join().unwrap().is_err());
    let fresh = sources
        .begin_ingestion_operation("fresh", JobProvider::Telegram)
        .unwrap();
    sources
        .with_ingestion_database(&fresh, |db| {
            assert!(db.read("late")?.is_none());
            db.save_ingestion(&modern("fresh", "telegram"))?;
            Ok(())
        })
        .unwrap();
    drop(fresh);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn application_shutdown_cancels_ingestion_before_any_late_database_write() {
    let (sources, dir) = local();
    let operation = sources
        .begin_ingestion_operation("slow-read", JobProvider::Telegram)
        .unwrap();
    sources.shutdown();
    assert!(operation.check().is_err());
    assert!(sources
        .with_ingestion_database(&operation, |db| db
            .save_ingestion(&modern("late", "telegram")))
        .is_err());
    drop(operation);
    drop(sources);
    if dir.exists() {
        std::fs::remove_dir_all(dir).unwrap();
    }
}
