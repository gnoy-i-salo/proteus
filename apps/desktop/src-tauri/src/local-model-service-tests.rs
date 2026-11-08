use super::*;
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    sync::Arc,
};
struct Temp(std::path::PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "proteus-model-service-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn budget() -> EmbeddingBudget {
    EmbeddingBudget {
        max_batch_tokens: 4096,
        max_batch_items: 8,
        max_resident_bytes: super::super::runtime::RESIDENT_RESERVATION_BYTES,
        max_duration_ms: 60000,
    }
}
#[test]
fn state_describes_pins_without_creating_a_cache_or_claiming_a_loaded_runtime() {
    let temp = Temp::new();
    let path = temp.0.join("missing");
    let service = LocalModelService::new(path.clone()).unwrap();
    let state = service.state();
    let descriptor = super::super::runtime::download_descriptor().unwrap();
    assert_eq!(state.model_id, descriptor.model_id);
    assert_eq!(state.model_revision, descriptor.revision);
    assert_eq!(
        state.package_bytes,
        descriptor.descriptor.weights.bytes
            + descriptor.descriptor.config.bytes
            + descriptor.descriptor.tokenizer.bytes
    );
    assert!(!state.runtime_loaded);
    assert!(!state.operation_active);
    assert_eq!(
        service.inspect("inspect", &|| Ok(())).unwrap(),
        PackageState::Missing
    );
    assert!(!path.exists());
    let wire = serde_json::to_value(state).unwrap();
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../src/shared/lib/fixtures/local-model-wire.json"
    ))
    .unwrap();
    assert_eq!(wire, fixture["state"]);
    assert_eq!(wire["schemaVersion"], 1);
    assert!(wire.get("cacheDir").is_none());
    assert!(!wire
        .to_string()
        .contains(&path.to_string_lossy().to_string()));
}

#[test]
fn download_recovery_removes_only_owned_orphans_and_preserves_verified_names() {
    let temp = Temp::new();
    let cache = temp.0.join("cache");
    let orphan = cache.join(".artifact-install-0000000000000001");
    std::fs::create_dir_all(&orphan).unwrap();
    std::fs::write(orphan.join("artifact"), b"partial model").unwrap();
    let published = cache.join("a".repeat(64));
    std::fs::write(&published, b"existing artifact").unwrap();
    let service = LocalModelService::new(cache).unwrap();
    service.recover_download(&|| Ok(())).unwrap();
    assert!(!orphan.exists());
    assert_eq!(std::fs::read(published).unwrap(), b"existing artifact");
    assert!(!service.state().runtime_loaded);
}

#[test]
fn download_recovery_preserves_cancellation_reason_and_hides_filesystem_details() {
    let temp = Temp::new();
    let path = temp.0.join("missing");
    let service = LocalModelService::new(path.clone()).unwrap();
    assert_eq!(
        service
            .recover_download(&|| Err("revoked exact".into()))
            .unwrap_err(),
        "revoked exact"
    );
    assert!(!path.exists());
    std::fs::write(&path, b"invalid root").unwrap();
    let reason = service.recover_download(&|| Ok(())).unwrap_err();
    assert!(!reason.contains(&path.to_string_lossy().to_string()));
    assert_eq!(std::fs::read(path).unwrap(), b"invalid root");
}
#[test]
fn cancelled_or_invalid_operations_fail_before_cache_io_and_download_network() {
    let temp = Temp::new();
    let path = temp.0.join("missing");
    let service = LocalModelService::new(path.clone()).unwrap();
    let revoked = || Err("source-revoked-exact".into());
    assert_eq!(
        service.inspect("inspect", &revoked).unwrap_err(),
        "source-revoked-exact"
    );
    assert_eq!(
        service
            .download("download", &revoked, &mut |_| panic!(
                "no progress after revocation"
            ))
            .unwrap_err(),
        "source-revoked-exact"
    );
    assert_eq!(
        service
            .with_runtime("load", budget(), &revoked, |_, _| Ok(()))
            .unwrap_err(),
        "source-revoked-exact"
    );
    assert!(service
        .inspect(" bad", &|| panic!("invalid id must fail before host check"))
        .is_err());
    assert!(!path.exists());
    assert!(!service.state().operation_active);
    assert!(!service.state().runtime_loaded);
}
#[test]
fn missing_or_corrupt_package_never_invokes_model_work_or_download_fallback() {
    let temp = Temp::new();
    let path = temp.0.join("cache");
    let service = LocalModelService::new(path.clone()).unwrap();
    assert!(service
        .with_runtime(
            "missing",
            budget(),
            &|| Ok(()),
            |_, _| -> Result<(), String> { panic!("missing weights cannot run") }
        )
        .is_err());
    assert!(!path.exists());
    std::fs::create_dir(&path).unwrap();
    let descriptor = super::super::runtime::cache_descriptor().unwrap();
    let artifact = path.join(descriptor.weights.sha256);
    std::fs::write(&artifact, b"corrupt existing bytes").unwrap();
    assert!(service.inspect("corrupt", &|| Ok(())).is_err());
    assert!(service
        .with_runtime(
            "corrupt-load",
            budget(),
            &|| Ok(()),
            |_, _| -> Result<(), String> { panic!("corrupt weights cannot run") }
        )
        .is_err());
    assert_eq!(std::fs::read(artifact).unwrap(), b"corrupt existing bytes");
    assert!(!service.state().runtime_loaded);
}
#[test]
fn resident_budget_is_enforced_before_any_source_check() {
    let temp = Temp::new();
    let service = LocalModelService::new(temp.0.join("cache")).unwrap();
    let mut small = budget();
    small.max_resident_bytes -= 1;
    assert!(service
        .with_runtime(
            "budget",
            small,
            &|| panic!("budget before source"),
            |_, _| -> Result<(), String> { panic!("budget before runtime") }
        )
        .is_err());
}
#[test]
fn cache_paths_are_owned_by_the_native_host() {
    assert!(LocalModelService::new(PathBuf::from("relative")).is_err());
    assert!(LocalModelService::new(std::env::temp_dir().join("x/../y")).is_err());
}
#[test]
fn shutdown_refuses_future_work_before_any_source_or_cache_access() {
    let temp = Temp::new();
    let cache = temp.0.join("cache");
    let service = LocalModelService::new(cache.clone()).unwrap();
    service.shutdown();
    assert!(service.inspect("closed", &|| Ok(())).is_err());
    assert!(service
        .download(
            "closed-download",
            &|| panic!("source check after shutdown"),
            &mut |_| panic!("progress after shutdown")
        )
        .is_err());
    assert!(service
        .with_runtime(
            "closed-runtime",
            budget(),
            &|| panic!("source check after shutdown"),
            |_, _| -> Result<(), String> { panic!("runtime after shutdown") }
        )
        .is_err());
    assert!(!cache.exists());
}
#[test]
fn inspection_download_and_inference_share_one_cancellable_operation() {
    use std::sync::mpsc;
    let temp = Temp::new();
    let service = Arc::new(LocalModelService::new(temp.0.join("cache")).unwrap());
    let (entered_tx, entered_rx) = mpsc::sync_channel(0);
    let (release_tx, release_rx) = mpsc::sync_channel(0);
    let worker_service = service.clone();
    let worker = std::thread::spawn(move || {
        let first = std::cell::Cell::new(true);
        worker_service.inspect("inspect", &|| {
            if first.replace(false) {
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            }
            Ok(())
        })
    });
    entered_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    assert!(service.state().operation_active);
    assert!(service
        .download("other", &|| panic!("busy"), &mut |_| panic!("busy"))
        .is_err());
    assert!(service
        .with_runtime(
            "load",
            budget(),
            &|| panic!("busy"),
            |_, _| -> Result<(), String> { panic!("busy") }
        )
        .is_err());
    assert!(service.unload().is_err());
    assert!(!service.cancel("wrong").unwrap());
    assert!(service.cancel("inspect").unwrap());
    assert!(service.state().operation_active);
    release_tx.send(()).unwrap();
    assert!(worker.join().unwrap().is_err());
    assert!(!service.state().operation_active);
    assert!(!service.cancel("inspect").unwrap());
    assert_eq!(
        service.inspect("retry", &|| Ok(())).unwrap(),
        PackageState::Missing
    );
    assert!(!service.unload().unwrap());
}
