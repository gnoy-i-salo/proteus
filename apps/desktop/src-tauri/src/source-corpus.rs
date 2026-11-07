use super::binding_policy::{
    validate_selection, BindingPeriod, SourceBinding, SourceBindingRequest, POLICY_VERSION,
};
use super::{
    document::document,
    grant::{iso, valid_id, validate_request},
    import,
    model::{
        Checkpoint, ImportedRecord, ImportedSource, Record, SourceBudget, SourceCoverage,
        SourceGrant, SourceGrantRequest,
    },
    store::SourceStore,
};
use crate::connections::{storage::random_id, Connections, Provider};
use chrono::Utc;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
pub(super) struct SourceInner {
    pub(super) directory: PathBuf,
    pub(super) connections: Connections,
    pub(super) store: SourceStore,
    pub(super) gate: Mutex<()>,
    pub(super) cancellations: Mutex<HashMap<String, Arc<AtomicBool>>>,
}
#[derive(Clone)]
pub struct Sources(pub(super) Arc<SourceInner>);
impl Sources {
    pub fn shutdown(&self) {
        if let Ok(flags) = self.0.cancellations.lock() {
            for flag in flags.values() {
                flag.store(true, Ordering::SeqCst);
            }
        }
    }
    pub fn new(directory: PathBuf, connections: Connections) -> Self {
        Self(Arc::new(SourceInner {
            store: SourceStore::new(connections.secrets()),
            directory,
            connections,
            gate: Mutex::new(()),
            cancellations: Mutex::new(HashMap::new()),
        }))
    }
    pub(super) fn path(&self, id: &str) -> Result<PathBuf, String> {
        if !valid_id(id) {
            return Err("Неверный идентификатор разрешения.".into());
        }
        Ok(self.0.directory.join(format!("{id}.enc")))
    }
    pub(super) fn imported(&self, resource: &str) -> Result<ImportedRecord, String> {
        let id = resource
            .strip_prefix("import:")
            .ok_or("Неверный импортированный ресурс.")?;
        self.0.store.read(&self.path(id)?)
    }
    pub(super) fn create(&self, request: SourceGrantRequest) -> Result<SourceGrant, String> {
        self.create_record(request, None).map(|record| record.grant)
    }
    pub(super) fn create_record(
        &self,
        mut request: SourceGrantRequest,
        selection: Option<SourceBindingRequest>,
    ) -> Result<Record, String> {
        if let Some(selection) = &selection {
            validate_selection(selection)?;
        }
        validate_request(&mut request)?;
        let _guard = self
            .0
            .gate
            .lock()
            .map_err(|_| "Ошибка состояния корпуса.")?;
        let account_id = if request.resource_id.starts_with("import:") {
            if self.imported(&request.resource_id)?.provider != request.provider {
                return Err("Импорт принадлежит другому сервису.".into());
            }
            "local-import".to_string()
        } else {
            let account = self.0.connections.account_id(request.provider)?;
            if request.provider == Provider::Instagram && request.resource_id != account {
                return Err("Instagram API читает только подключённый профессиональный аккаунт. Для другого материала используйте импорт.".into());
            }
            if request.provider == Provider::Telegram && request.resource_id.starts_with('@') {
                let chat = self.0.connections.telegram_request(serde_json::json!({"@type":"searchPublicChat","username":request.resource_id.trim_start_matches('@')}))?;
                request.resource_id = chat
                    .get("id")
                    .and_then(serde_json::Value::as_i64)
                    .ok_or("Telegram не подтвердил чат.")?
                    .to_string();
            }
            account
        };
        let now = Utc::now();
        let grant = SourceGrant {
            request,
            id: random_id(18)?,
            snapshot_id: random_id(18)?,
            account_id,
            expires_at: iso(now.timestamp() + 86400).ok_or("Ошибка времени.")?,
        };
        let mut limitations = vec!["Полученный текст подтверждает происхождение материала, но не истинность его содержания.".into(), "Вложения без распознавания не считаются изученными.".into()];
        if grant.request.provider != Provider::Github
            || grant.request.resource_id.starts_with("import:")
        {
            limitations.push("Один выбранный разговор или источник используется как reference; независимая holdout-выборка не сформирована.".into());
        }
        if grant.request.author_id.is_some() && selection.is_none() {
            limitations.push("Ответы других участников исключены фильтром автора; контекст реплик может быть неполным.".into());
        }
        if (grant.request.from.is_some() || grant.request.to.is_some())
            && selection
                .as_ref()
                .is_none_or(|scope| !scope.include_undated)
        {
            limitations
                .push("Материал без подтверждённого времени исключён выбранным периодом.".into());
        }
        let binding = selection.map(|selection| SourceBinding {
            schema_version: 1,
            id: format!("binding:{}", grant.id),
            persona_id: selection.persona_id,
            provider: grant.request.provider,
            account_id: Some(grant.account_id.clone()),
            resource_id: grant.request.resource_id.clone(),
            grant_id: grant.id.clone(),
            policy_version: POLICY_VERSION.into(),
            role: selection.role,
            target_author_id: grant.request.author_id.clone(),
            allowed_context_author_ids: selection.allowed_context_author_ids,
            allow_unknown_context: selection.allow_unknown_context,
            period: BindingPeriod {
                from: grant.request.from.clone(),
                to: grant.request.to.clone(),
                include_undated: selection.include_undated,
            },
        });
        let record = Record {
            binding,
            grant: grant.clone(),
            documents: vec![],
            coverage: SourceCoverage::default(),
            cursor: None,
            checkpoint: Checkpoint::default(),
            steps: 0,
            started_at: None,
            cancelled: false,
            limitations,
        };
        self.0.store.write(&self.path(&grant.id)?, &record)?;
        Ok(record)
    }
    pub(super) fn flag(&self, id: &str) -> Result<Arc<AtomicBool>, String> {
        self.path(id)?;
        Ok(self
            .0
            .cancellations
            .lock()
            .map_err(|_| "Ошибка отмены источника.")?
            .entry(id.into())
            .or_insert_with(|| Arc::new(AtomicBool::new(false)))
            .clone())
    }
    pub(super) fn clear(&self, provider: Provider) -> Result<(), String> {
        // Serialize enumeration with creation/import as well as deletion. Otherwise an
        // import finishing between scan and lock could survive a successful clear.
        let _guard = self
            .0
            .gate
            .lock()
            .map_err(|_| "Ошибка состояния корпуса.")?;
        let mut ids: HashSet<String> = self.0.store.ids(&self.0.directory)?.into_iter().collect();
        for directory in [
            &self.0.directory,
            &self.0.directory.join("v2"),
            &self.0.directory.join("v3"),
        ] {
            let entries = match std::fs::read_dir(directory) {
                Ok(entries) => entries,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => return Err("Не удалось открыть каталог корпуса.".into()),
            };
            for entry in entries {
                let entry = entry.map_err(|_| "Не удалось прочитать каталог корпуса.")?;
                let path = entry.path();
                if path.extension().and_then(|v| v.to_str()) != Some("enc") {
                    continue;
                }
                let Some(id) = path
                    .file_stem()
                    .and_then(|v| v.to_str())
                    .filter(|v| valid_id(v))
                else {
                    continue;
                };
                if !entry
                    .file_type()
                    .map_err(|_| "Не удалось проверить файл корпуса.")?
                    .is_file()
                {
                    continue;
                }
                ids.insert(id.to_string());
            }
        }
        let mut selected = Vec::new();
        for id in ids {
            if self.0.store.owner(&self.path(&id)?)? == provider {
                self.flag(&id)?.store(true, Ordering::SeqCst);
                selected.push(id);
            }
        }
        for id in selected {
            self.0.store.remove(&self.path(&id)?)?;
        }
        Ok(())
    }
    pub(super) fn delete(&self, id: &str) -> Result<(), String> {
        let path = self.path(id)?;
        self.flag(id)?.store(true, Ordering::SeqCst);
        let _guard = self
            .0
            .gate
            .lock()
            .map_err(|_| "Ошибка состояния корпуса.")?;
        if !self.0.store.is_deleted(&path)? {
            let record: Record = self.0.store.read(&path)?;
            if record.grant.id != id {
                return Err("Корпус не соответствует разрешению.".into());
            }
        }
        self.0.store.remove(&path)
    }
    pub(super) fn import(&self, provider: Provider, path: &Path) -> Result<ImportedSource, String> {
        let metadata = std::fs::metadata(path).map_err(|_| "Не удалось открыть выбранный файл.")?;
        if !metadata.is_file() || metadata.len() > 16 * 1024 * 1024 {
            return Err("Нужен JSON или текстовый файл до 16 МиБ.".into());
        }
        let data = std::fs::read_to_string(path)
            .map_err(|_| "Нужен текстовый экспорт в UTF-8.".to_string())?;
        let (items, warnings) = import::parse(
            provider,
            &data,
            path.extension().and_then(|ext| ext.to_str()).unwrap_or(""),
        )?;
        let _guard = self
            .0
            .gate
            .lock()
            .map_err(|_| "Ошибка состояния корпуса.")?;
        let id = random_id(18)?;
        let resource = format!("import:{id}");
        let request = SourceGrantRequest {
            provider,
            resource_id: resource.clone(),
            author_id: None,
            from: None,
            to: None,
            allow_model_processing: false,
            operations: vec![],
            budget: SourceBudget {
                max_steps: 1,
                max_documents: 10000,
                max_bytes: 16777216,
                max_duration_ms: 1000,
            },
        };
        let documents: Vec<_> = items
            .into_iter()
            .map(|item| document(&request, item, false))
            .collect();
        if documents.len() > 10000 {
            return Err("Экспорт содержит больше 10000 объектов. Выберите меньший период.".into());
        }
        let count = documents.len();
        self.0.store.write(
            &self.path(&id)?,
            &ImportedRecord {
                provider,
                documents,
                warnings: warnings.clone(),
            },
        )?;
        Ok(ImportedSource {
            id: resource,
            label: path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("Импорт")
                .chars()
                .filter(|c| !c.is_control())
                .take(128)
                .collect(),
            document_count: count,
            warnings,
        })
    }
}
pub(super) fn remove_file(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => crate::connections::storage::sync_directory(
            path.parent().ok_or("Неверный путь корпуса.")?,
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("Не удалось удалить выбранный корпус.".into()),
    }
}
