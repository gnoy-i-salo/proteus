use crate::corpus_database::JobProvider;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
const MAX_ACTIVE: usize = 16;
#[derive(Default)]
struct State {
    entries: Vec<(String, JobProvider, Arc<AtomicBool>)>,
    clearing: Vec<JobProvider>,
    stopped: bool,
}
#[derive(Clone, Default)]
pub struct Operations(Arc<Mutex<State>>);
pub struct Operation {
    registry: Operations,
    id: String,
    flag: Arc<AtomicBool>,
}
pub struct Clearing {
    registry: Operations,
    provider: JobProvider,
}
impl Operations {
    pub fn begin(&self, id: &str, provider: JobProvider) -> Result<Operation, String> {
        let mut state = self.0.lock().map_err(|_| "Ошибка состояния операций.")?;
        if id.is_empty()
            || id.len() > 128
            || state
                .entries
                .iter()
                .any(|entry| entry.0 == id && !entry.2.load(Ordering::SeqCst))
        {
            return Err("Неверный или занятый идентификатор операции.".into());
        }
        if state.stopped || state.clearing.contains(&provider) || state.entries.len() >= MAX_ACTIVE
        {
            return Err(
                "Операции временно недоступны: очистка, завершение или предел активных операций."
                    .into(),
            );
        }
        let flag = Arc::new(AtomicBool::new(false));
        state.entries.push((id.into(), provider, flag.clone()));
        Ok(Operation {
            registry: self.clone(),
            id: id.into(),
            flag,
        })
    }
    pub fn clear(&self, provider: JobProvider) -> Result<Clearing, String> {
        let mut state = self.0.lock().map_err(|_| "Ошибка состояния операций.")?;
        if state.stopped || state.clearing.contains(&provider) {
            return Err("Очистка уже выполняется или приложение завершается.".into());
        }
        state.clearing.push(provider);
        for (_, owner, flag) in &state.entries {
            if *owner == provider {
                flag.store(true, Ordering::SeqCst);
            }
        }
        Ok(Clearing {
            registry: self.clone(),
            provider,
        })
    }
    pub fn cancel(&self, id: &str) -> Result<(), String> {
        let state = self.0.lock().map_err(|_| "Ошибка состояния операций.")?;
        for (current, _, flag) in &state.entries {
            if current == id {
                flag.store(true, Ordering::SeqCst);
            }
        }
        Ok(())
    }
    pub fn shutdown(&self) {
        if let Ok(mut state) = self.0.lock() {
            state.stopped = true;
            for (_, _, flag) in &state.entries {
                flag.store(true, Ordering::SeqCst);
            }
        }
    }
}
impl Operation {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn shutdown_requested(&self) -> Result<bool, String> {
        Ok(self
            .registry
            .0
            .lock()
            .map_err(|_| "Ошибка состояния операций.")?
            .stopped)
    }
    pub fn is_current(&self) -> Result<bool, String> {
        let state = self
            .registry
            .0
            .lock()
            .map_err(|_| "Ошибка состояния операций.")?;
        Ok(state
            .entries
            .iter()
            .rev()
            .find(|entry| entry.0 == self.id)
            .is_some_and(|entry| Arc::ptr_eq(&entry.2, &self.flag)))
    }
    pub fn cancelled(&self) -> &AtomicBool {
        &self.flag
    }
    pub fn check(&self) -> Result<(), String> {
        if self.flag.load(Ordering::SeqCst) {
            Err("Операция отменена.".into())
        } else {
            Ok(())
        }
    }
}
impl Drop for Operation {
    fn drop(&mut self) {
        if let Ok(mut state) = self.registry.0.lock() {
            // Cancelled workers may retire after a replacement starts. Keep each
            // registration until its own guard drops, so capacity stays bounded.
            state
                .entries
                .retain(|entry| entry.0 != self.id || !Arc::ptr_eq(&entry.2, &self.flag));
        }
    }
}
impl Drop for Clearing {
    fn drop(&mut self) {
        if let Ok(mut state) = self.registry.0.lock() {
            state.clearing.retain(|provider| *provider != self.provider);
        }
    }
}
#[cfg(test)]
#[path = "ingestion-operations-tests.rs"]
mod tests;
