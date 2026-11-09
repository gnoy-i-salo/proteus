//! Rehydrate persisted memory only through live, budgeted reference episodes.
//! The storage hash proves identity, not permission or source truth. Every read
//! uses current grants/index/period and verifies support against exact originals.
use super::{MemorySession, RecordedRetrieval};
use crate::{
    corpus_database::memory_store::MemoryBundle,
    ingestion::grants::GrantUse,
    memory::types::{self, MemoryItem},
};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    sync::Arc,
};
const INVALID: &str = "Опубликованная память не соответствует разрешённой области сессии.";

pub struct AuthorizedMemory {
    pub(super) memory_snapshot_id: String,
    pub(super) items: Vec<MemoryItem>,
    pub(super) receipts: Vec<Arc<RecordedRetrieval>>,
}
impl AuthorizedMemory {
    pub fn memory_snapshot_id(&self) -> &str {
        &self.memory_snapshot_id
    }
    pub fn items(&self) -> &[MemoryItem] {
        &self.items
    }
    pub fn receipts(&self) -> &[Arc<RecordedRetrieval>] {
        &self.receipts
    }
}
impl MemorySession {
    pub fn read_published_memory(
        &mut self,
        snapshot_id: &str,
        item_ids: &[String],
        now: impl Fn() -> i64,
    ) -> Result<AuthorizedMemory, String> {
        types::validate_host_id(snapshot_id, "снимок опубликованной памяти")?;
        types::validate_unique_host_ids(item_ids, "элемент опубликованной памяти", 1000)?;
        self.verify_current(&now)?;
        let bundle = self.load_published_memory(snapshot_id, &now)?;
        let mut by_id: HashMap<_, _> = bundle
            .items
            .into_iter()
            .map(|item| (item.id.clone(), item))
            .collect();
        let mut items = Vec::with_capacity(item_ids.len());
        for id in item_ids {
            let item = by_id.remove(id).ok_or(INVALID)?;
            if item.policy_version != self.scope.policy_version()? {
                return Err(INVALID.into());
            }
            items.push(item);
        }
        let mut seen = HashSet::new();
        let mut receipts = Vec::new();
        // Preserve first occurrence order and read each episode once per request.
        // Repeated calls still consume budget; no stale authority cache is added.
        for item in &items {
            for episode in &item.episode_ids {
                if seen.insert(episode) {
                    receipts.push(self.read_episode_recorded(episode, &now)?);
                }
            }
        }
        crate::memory::research_support::verify(&items, &receipts, &|| {
            self.scope.guard(&now).map(|_| ())
        })?;
        self.verify_current(now)?;
        Ok(AuthorizedMemory {
            memory_snapshot_id: bundle.snapshot.id,
            items,
            receipts,
        })
    }

    fn load_published_memory(
        &mut self,
        snapshot_id: &str,
        now: &dyn Fn() -> i64,
    ) -> Result<MemoryBundle, String> {
        let scope = &self.scope;
        let ledger = RefCell::new(&mut self.ledger);
        let models = RefCell::new(self.models.as_mut());
        // The source gate freezes grant/index mutations while the storage reader
        // validates the immutable bundle. Checkpoints must not reacquire the gate.
        scope
            .sources
            .with_ingestion_database(&scope.operation, |db| {
                let clock = || {
                    scope.operation.check()?;
                    let time = now();
                    scope.operation.check()?;
                    if time < 0 || time >= scope.deadline {
                        return Err("Разрешение источника истекло.".into());
                    }
                    Ok(time)
                };
                let check = || {
                    ledger.borrow_mut().verify(clock)?;
                    if let Some(models) = models.borrow_mut().as_mut() {
                        models.verify(&clock)?;
                    }
                    Ok(())
                };
                check()?;
                scope.authority.current(db, clock()?, GrantUse::Model)?;
                let pin = scope
                    .authority
                    .pin_index(db, &scope.source.snapshot_id, &check)?;
                if pin.receipt != scope.source {
                    return Err(INVALID.into());
                }
                let bundle =
                    db.read_memory_snapshot_checked(snapshot_id, &scope.persona_id, &check)?;
                if bundle.snapshot.corpus_snapshot_id != scope.source.snapshot_id {
                    return Err(INVALID.into());
                }
                check()?;
                Ok(bundle)
            })
    }
}
