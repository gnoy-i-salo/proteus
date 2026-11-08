//! Opaque host-owned session handles. Model actions cannot create or widen scope.
use super::session::MemorySession;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, Weak,
    },
};
const MAX_SESSIONS: usize = 16;
const UNAVAILABLE: &str = "Сессия памяти недоступна.";
struct Entry {
    session: Mutex<MemorySession>,
    cancelled: Arc<AtomicBool>,
}
#[derive(Default)]
struct State {
    entries: HashMap<String, Arc<Entry>>,
    // Closed workers still occupy capacity until they release their resources.
    live: Vec<Weak<Entry>>,
    stopped: bool,
}
#[derive(Default)]
pub struct MemorySessions {
    state: Mutex<State>,
}
impl MemorySessions {
    pub fn insert(
        &self,
        mut session: MemorySession,
        now: impl Fn() -> i64,
    ) -> Result<String, String> {
        session.verify_current(now)?;
        let id = format!(
            "memorySession:{}",
            crate::connections::storage::random_id(24)?
        );
        let entry = Arc::new(Entry {
            cancelled: session.cancellation_handle(),
            session: Mutex::new(session),
        });
        let mut state = self.state.lock().map_err(|_| UNAVAILABLE)?;
        state.live.retain(|entry| entry.strong_count() != 0);
        if state.stopped || state.live.len() >= MAX_SESSIONS || state.entries.contains_key(&id) {
            return Err(UNAVAILABLE.into());
        }
        state.live.push(Arc::downgrade(&entry));
        state.entries.insert(id.clone(), entry);
        Ok(id)
    }
    pub fn with_session<R>(
        &self,
        id: &str,
        now: impl Fn() -> i64,
        work: impl FnOnce(&mut MemorySession) -> Result<R, String>,
    ) -> Result<R, String> {
        if id.is_empty() || id.len() > 128 {
            return Err(UNAVAILABLE.into());
        }
        let entry = self
            .state
            .lock()
            .map_err(|_| UNAVAILABLE)?
            .entries
            .get(id)
            .cloned()
            .ok_or(UNAVAILABLE)?;
        // Never queue model actions behind a busy session or hold the map during work.
        let mut session = entry.session.try_lock().map_err(|_| UNAVAILABLE)?;
        session.verify_current(&now)?;
        let result = work(&mut session);
        let checked = session.verify_current(now);
        let value = result?;
        checked?;
        let state = self.state.lock().map_err(|_| UNAVAILABLE)?;
        if state.stopped
            || !state
                .entries
                .get(id)
                .is_some_and(|current| Arc::ptr_eq(current, &entry))
        {
            return Err(UNAVAILABLE.into());
        }
        Ok(value)
    }
    pub fn close(&self, id: &str) -> Result<bool, String> {
        let entry = self
            .state
            .lock()
            .map_err(|_| UNAVAILABLE)?
            .entries
            .remove(id);
        if let Some(entry) = entry {
            entry.cancelled.store(true, Ordering::SeqCst);
            Ok(true)
        } else {
            Ok(false)
        }
    }
    pub fn shutdown(&self) {
        // A poisoned registry must still cancel workers; admission fails closed.
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.stopped = true;
        for entry in state.live.iter().filter_map(Weak::upgrade) {
            entry.cancelled.store(true, Ordering::SeqCst);
        }
        state.entries.clear();
    }
}
impl Drop for MemorySessions {
    fn drop(&mut self) {
        self.shutdown();
    }
}
