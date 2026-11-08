//! SQLCipher-backed source records with explicit read-only legacy generations.
#[cfg(test)]
use super::model::{ImportedRecord, Record};
use super::{
    model::SourceDocument,
    storage_codec::{current, encode, normalize, Deleted, StoredSource, FORMAT, INVALID, VERSION},
};
use crate::connections::{
    storage::{EncryptedStore, SecretStore},
    Provider,
};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
pub(super) fn current_path(legacy: &Path) -> Result<PathBuf, String> {
    Ok(legacy
        .parent()
        .ok_or(INVALID)?
        .join("v3")
        .join(legacy.file_name().ok_or(INVALID)?))
}
pub(super) fn previous_path(legacy: &Path) -> Result<PathBuf, String> {
    Ok(legacy
        .parent()
        .ok_or(INVALID)?
        .join("v2")
        .join(legacy.file_name().ok_or(INVALID)?))
}
pub(super) struct SourceStore {
    legacy: EncryptedStore,
    secrets: Arc<dyn SecretStore>,
    database: std::sync::Mutex<Option<(PathBuf, crate::corpus_database::CorpusDatabase)>>,
}
impl SourceStore {
    pub fn new(secrets: Arc<dyn SecretStore>) -> Self {
        Self {
            legacy: EncryptedStore::new(secrets.clone()),
            secrets,
            database: std::sync::Mutex::new(None),
        }
    }
    pub(super) fn with_database<T>(
        &self,
        directory: &Path,
        action: impl FnOnce(&mut crate::corpus_database::CorpusDatabase) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut slot = self
            .database
            .lock()
            .map_err(|_| "Ошибка состояния базы корпуса.")?;
        if slot.is_none() {
            let mut database = crate::corpus_database::CorpusDatabase::open_with_secrets(
                &directory.join("metadata.db"),
                self.secrets.as_ref(),
            )?;
            // Recover once per newly opened connection, never on a later read
            // that could otherwise pause a worker running in this process.
            database.recover_interrupted_jobs()?;
            // Native rebuilds own fresh stages and never resume them after a
            // process restart. This connection is not exposed to workers yet.
            // Other producers and durable publication receipts remain intact.
            loop {
                let abandoned = database.search_staging_ids("nativeFts:", None, 100)?;
                if abandoned.is_empty() {
                    break;
                }
                database.discard_search_staging(&abandoned)?;
            }
            *slot = Some((directory.to_path_buf(), database));
        }
        let (owner, database) = slot.as_mut().ok_or(INVALID)?;
        if owner != directory {
            return Err("База корпуса принадлежит другому каталогу.".into());
        }
        action(database)
    }
    fn legacy_value(&self, path: &Path) -> Result<Option<(Value, u8, String)>, String> {
        for (generation, candidate) in [
            (3, current_path(path)?),
            (2, previous_path(path)?),
            (1, path.to_path_buf()),
        ] {
            match std::fs::symlink_metadata(&candidate) {
                Ok(_) => {
                    let (value, hash): (Value, String) = self.legacy.read_verified(&candidate)?;
                    let canonical = if generation > 1 {
                        if value["version"] != generation {
                            return Err(INVALID.into());
                        }
                        normalize(value, true)?
                    } else {
                        normalize(value, false)?
                    };
                    let generation = if generation == 1 && canonical["kind"] == "import" {
                        0
                    } else {
                        generation
                    };
                    return Ok(Some((canonical, generation, hash)));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err("Не удалось открыть исходное поколение корпуса.".into()),
            }
        }
        Ok(None)
    }
    fn load(
        &self,
        path: &Path,
        database: &mut crate::corpus_database::CorpusDatabase,
    ) -> Result<Option<crate::corpus_database::StoredRecord>, String> {
        let id = record_id(path)?;
        if let Some(record) = database.read(id)? {
            if record.namespace != crate::corpus_database::RecordNamespace::SourceService {
                return Err(INVALID.into());
            }
            return Ok(Some(record));
        }
        let Some((canonical, generation, ciphertext_sha256)) = self.legacy_value(path)? else {
            return Ok(None);
        };
        if canonical["kind"] == "deleted" {
            let deleted: Deleted = current(canonical)?;
            database.revoke(id, deleted.provider.key())?;
        } else {
            let input = database_input(id, canonical, Some(generation), database)?;
            let audit = crate::corpus_database::MigrationAudit {
                source_generation: generation,
                ciphertext_sha256,
                document_count: input.documents.len(),
                canonical_sha256: crate::corpus_database::record_hash(&input),
            };
            database.migrate(&input, &audit)?;
            if database.audit(id)?.as_ref() != Some(&audit) {
                return Err(INVALID.into());
            }
        }
        database.read(id)
    }
    pub fn read<T: StoredSource>(&self, path: &Path) -> Result<T, String> {
        self.with_database(path.parent().ok_or(INVALID)?, |database| {
            let record = self.load(path, database)?.ok_or("Корпус не найден.")?;
            current(database_value(record)?)
        })
    }
    pub fn write<T: StoredSource>(&self, path: &Path, value: &T) -> Result<(), String> {
        let canonical = encode(value)?;
        self.with_database(path.parent().ok_or(INVALID)?, |database| {
            self.load(path, database)?;
            if canonical["kind"] == "deleted" {
                let deleted: Deleted = current(canonical)?;
                database.revoke(record_id(path)?, deleted.provider.key())
            } else {
                let input = database_input(record_id(path)?, canonical, None, database)?;
                database.save(&input)?;
                Ok(())
            }
        })
    }
    pub fn owner(&self, path: &Path) -> Result<Provider, String> {
        self.with_database(path.parent().ok_or(INVALID)?, |database| {
            let record = self.load(path, database)?.ok_or("Корпус не найден.")?;
            serde_json::from_value(Value::String(record.input.provider)).map_err(|_| INVALID.into())
        })
    }
    pub fn is_deleted(&self, path: &Path) -> Result<bool, String> {
        self.with_database(path.parent().ok_or(INVALID)?, |database| {
            Ok(self
                .load(path, database)?
                .is_some_and(|record| record.input.kind == "deleted"))
        })
    }
    pub fn references(
        &self,
        path: &Path,
    ) -> Result<Vec<crate::corpus_database::RevisionReference>, String> {
        self.with_database(path.parent().ok_or(INVALID)?, |database| {
            let record = self.load(path, database)?.ok_or("Корпус не найден.")?;
            if record.input.kind == "deleted" {
                return Err("Источник отозван.".into());
            }
            Ok(record.references)
        })
    }
    pub fn ids(&self, directory: &Path) -> Result<Vec<String>, String> {
        if !directory
            .join("metadata.db")
            .try_exists()
            .map_err(|_| INVALID)?
        {
            return Ok(vec![]);
        }
        self.with_database(directory, |database| {
            database.ids_in_namespace(crate::corpus_database::RecordNamespace::SourceService)
        })
    }
    pub fn remove(&self, path: &Path) -> Result<(), String> {
        let provider = self.owner(path)?;
        self.write(path, &Deleted { provider })?;
        // The SQL tombstone is durable before unlink and retained after successful
        // cleanup, so preserved old ciphertext can never revive a revoked source.
        super::corpus::remove_file(path)?;
        super::corpus::remove_file(&previous_path(path)?)?;
        super::corpus::remove_file(&current_path(path)?)
    }
}
fn record_id(path: &Path) -> Result<&str, String> {
    path.file_stem()
        .and_then(|id| id.to_str())
        .filter(|id| !id.is_empty() && id.len() <= 256)
        .ok_or_else(|| INVALID.into())
}
fn database_input(
    id: &str,
    canonical: Value,
    legacy_generation: Option<u8>,
    database: &crate::corpus_database::CorpusDatabase,
) -> Result<crate::corpus_database::RecordInput, String> {
    let kind = canonical["kind"].as_str().ok_or(INVALID)?.to_owned();
    let mut payload = canonical["payload"].as_object().ok_or(INVALID)?.clone();
    let documents = payload
        .remove("documents")
        .and_then(|value| value.as_array().cloned())
        .ok_or(INVALID)?;
    let (provider, account) = if kind == "grant" {
        let grant = payload.get("grant").ok_or(INVALID)?;
        (
            grant["provider"].as_str().ok_or(INVALID)?.to_owned(),
            grant["accountId"].as_str().ok_or(INVALID)?.to_owned(),
        )
    } else {
        (
            payload
                .get("provider")
                .and_then(Value::as_str)
                .ok_or(INVALID)?
                .to_owned(),
            "local-import".into(),
        )
    };
    let mut revisions = Vec::new();
    for document in documents {
        let stored: SourceDocument =
            serde_json::from_value(document.clone()).map_err(|_| INVALID)?;
        let source_key = serde_json::to_string(&(
            stored.provider.key(),
            account.as_str(),
            stored.resource_id.as_str(),
            stored.id.as_str(),
        ))
        .map_err(|_| INVALID)?;
        let document_json = document.to_string();
        let acquisition = if kind == "import" || stored.resource_id.starts_with("import:") {
            crate::corpus_database::Acquisition::Archive
        } else {
            crate::corpus_database::Acquisition::Api
        };
        let provenance = if let Some(generation) = legacy_generation {
            crate::corpus_database::SourceProvenance::legacy(acquisition, generation)?
        } else if let Some(existing) = database.matching_revision(
            crate::corpus_database::RecordNamespace::SourceService,
            &source_key,
            &document_json,
        )? {
            existing.provenance
        } else {
            crate::corpus_database::SourceProvenance::source_service(acquisition)
        };
        revisions.push(crate::corpus_database::RevisionInput {
            source_key,
            document_id: stored.id,
            document_json,
            provenance,
        });
    }
    Ok(crate::corpus_database::RecordInput {
        id: id.into(),
        kind,
        provider,
        state_json: Value::Object(payload).to_string(),
        documents: revisions,
    })
}
fn database_value(record: crate::corpus_database::StoredRecord) -> Result<Value, String> {
    let mut payload: Value = serde_json::from_str(&record.input.state_json).map_err(|_| INVALID)?;
    if record.input.kind == "deleted" {
        payload = serde_json::json!({"provider":record.input.provider});
    } else {
        let documents = record
            .input
            .documents
            .into_iter()
            .map(|document| {
                serde_json::from_str::<Value>(&document.document_json)
                    .map_err(|_| INVALID.to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        payload
            .as_object_mut()
            .ok_or(INVALID)?
            .insert("documents".into(), Value::Array(documents));
    }
    Ok(
        serde_json::json!({"format":FORMAT,"version":VERSION,"kind":record.input.kind,"payload":payload}),
    )
}

#[cfg(test)]
#[path = "source-store-tests.rs"]
mod tests;
