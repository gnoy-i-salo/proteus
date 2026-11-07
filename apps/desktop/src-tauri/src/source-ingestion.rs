//! Short database actions share the legacy SourceStore connection and gate.
//! External reads happen outside this gate; a cancelled operation cannot persist
//! its result after a provider clear has completed.
use super::Sources;
use crate::corpus_database::{CorpusDatabase, JobProvider};
use crate::ingestion::operations::Operation;
impl Sources {
    pub fn begin_ingestion_operation(
        &self,
        id: &str,
        provider: JobProvider,
    ) -> Result<Operation, String> {
        self.0.ingestion_operations.begin(id, provider)
    }
    pub fn cancel_ingestion_operation(&self, id: &str) -> Result<(), String> {
        self.0.ingestion_operations.cancel(id)
    }
    pub fn with_ingestion_database<T>(
        &self,
        operation: &Operation,
        action: impl FnOnce(&mut CorpusDatabase) -> Result<T, String>,
    ) -> Result<T, String> {
        let _guard = self
            .0
            .gate
            .lock()
            .map_err(|_| "Ошибка состояния корпуса.")?;
        operation.check()?;
        self.0.store.with_database(&self.0.directory, action)
    }
}

pub(super) fn ingestion_provider(provider: crate::connections::Provider) -> JobProvider {
    match provider {
        crate::connections::Provider::Telegram => JobProvider::Telegram,
        crate::connections::Provider::Vk => JobProvider::Vk,
        crate::connections::Provider::Github => JobProvider::Github,
        crate::connections::Provider::Instagram => JobProvider::Instagram,
    }
}
