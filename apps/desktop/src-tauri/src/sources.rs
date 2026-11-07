use crate::connections::Provider;
use crate::connections::{storage::random_id, Connections};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tauri_plugin_dialog::DialogExt;
#[path = "source-import.rs"]
mod import;
#[path = "source-storage-codec.rs"]
mod storage_codec;
use storage_codec::SourceStore;
#[path = "source-readers.rs"]
mod readers;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceBudget {
    pub max_steps: u32,
    pub max_documents: u32,
    pub max_bytes: u64,
    pub max_duration_ms: u64,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceGrantRequest {
    pub provider: Provider,
    pub resource_id: String,
    pub author_id: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub allow_model_processing: bool,
    pub operations: Vec<String>,
    pub budget: SourceBudget,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceGrant {
    #[serde(flatten)]
    pub request: SourceGrantRequest,
    pub id: String,
    pub account_id: String,
    pub snapshot_id: String,
    pub expires_at: String,
}
#[cfg_attr(test, derive(Serialize))]
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SourceAction {
    Describe,
    Coverage,
    Search {
        query: String,
        limit: usize,
    },
    Collect {
        cursor: Option<String>,
        limit: usize,
    },
    Read {
        #[serde(rename = "documentIds")]
        document_ids: Vec<String>,
    },
}
impl SourceAction {
    fn operation(&self) -> &'static str {
        match self {
            Self::Describe => "describe",
            Self::Coverage => "coverage",
            Self::Search { .. } => "search",
            Self::Collect { .. } => "collect",
            Self::Read { .. } => "read",
        }
    }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceDocument {
    pub id: String,
    pub provider: Provider,
    pub resource_id: String,
    pub author_id: Option<String>,
    pub author_label: Option<String>,
    pub authorship: String,
    pub kind: String,
    pub mode: String,
    pub text: String,
    pub source_url: Option<String>,
    pub created_at: Option<String>,
    pub retrieved_at: Option<String>,
    pub content_hash: String,
    pub reply_to: Option<String>,
    pub is_forwarded: bool,
    pub truncated: bool,
    pub partition: String,
}
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceCoverage {
    pub documents: u64,
    pub total_documents: Option<u64>,
    pub bytes: u64,
    pub complete: bool,
    pub truncated: bool,
    pub not_analyzed: u64,
    pub unavailable: Vec<String>,
}
#[cfg_attr(test, derive(Deserialize))]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceResult {
    pub snapshot_id: String,
    pub documents: Vec<SourceDocument>,
    pub coverage: SourceCoverage,
    pub cursor: Option<String>,
    pub limitations: Vec<String>,
}
#[cfg_attr(test, derive(Deserialize))]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedSource {
    pub id: String,
    pub label: String,
    pub document_count: usize,
    pub warnings: Vec<String>,
}

pub(crate) fn timestamp(text: &str) -> Result<i64, String> {
    if let Ok(value) = DateTime::parse_from_rfc3339(text) {
        return Ok(value.timestamp());
    }
    if let Ok(value) = DateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%z") {
        return Ok(value.timestamp());
    }
    NaiveDate::parse_from_str(text, "%Y-%m-%d")
        .ok()
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map(|date| date.and_utc().timestamp())
        .ok_or_else(|| "Неверный формат даты.".into())
}
pub(crate) fn iso(seconds: i64) -> Option<String> {
    DateTime::<Utc>::from_timestamp(seconds, 0).map(|value| value.to_rfc3339())
}
fn validate_request(request: &mut SourceGrantRequest) -> Result<(), String> {
    if !request.allow_model_processing {
        return Err(
            "Подтвердите передачу выбранного материала настроенному модельному провайдеру.".into(),
        );
    }
    let budget = &request.budget;
    if !(1..=256).contains(&budget.max_steps)
        || !(1..=10000).contains(&budget.max_documents)
        || !(1024..=16 * 1024 * 1024).contains(&budget.max_bytes)
        || !(1000..=1800000).contains(&budget.max_duration_ms)
    {
        return Err("Бюджет источника выходит за допустимые пределы.".into());
    }
    if request.operations.is_empty()
        || request.operations.len() > 5
        || request
            .operations
            .iter()
            .any(|op| !["describe", "coverage", "search", "collect", "read"].contains(&op.as_str()))
        || request
            .operations
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != request.operations.len()
    {
        return Err("Недопустимые операции источника.".into());
    }
    if request
        .author_id
        .as_ref()
        .is_some_and(|id| id.is_empty() || id.len() > 128 || id.chars().any(char::is_control))
    {
        return Err("Неверный идентификатор автора.".into());
    }
    let from = request.from.as_deref().map(timestamp).transpose()?;
    let to = request
        .to
        .as_deref()
        .map(|text| {
            timestamp(text).map(|seconds| seconds + if text.len() == 10 { 86399 } else { 0 })
        })
        .transpose()?;
    if from.zip(to).is_some_and(|(from, to)| from > to) {
        return Err("Начало периода позже окончания.".into());
    }
    request.from = from.and_then(iso);
    request.to = to.and_then(iso);
    request.resource_id = canonical_resource(request.provider, &request.resource_id)?;
    Ok(())
}
fn canonical_resource(provider: Provider, raw: &str) -> Result<String, String> {
    let invalid = || "Нужна ссылка или идентификатор одного поддерживаемого ресурса.".to_string();
    if raw.len() > 512
        || raw.contains('%')
        || raw.chars().any(char::is_whitespace)
        || raw.split('/').any(|part| matches!(part, "." | ".."))
    {
        return Err(invalid());
    }
    if let Some(id) = raw.strip_prefix("import:") {
        return if valid_id(id) {
            Ok(raw.into())
        } else {
            Err(invalid())
        };
    }
    let resource = if raw.starts_with("https://") {
        let url = url::Url::parse(raw).map_err(|_| invalid())?;
        let hosts: &[&str] = match provider {
            Provider::Github => &["github.com", "www.github.com"],
            Provider::Vk => &["vk.com", "www.vk.com", "vk.ru", "www.vk.ru"],
            Provider::Telegram => &["t.me"],
            Provider::Instagram => &[],
        };
        if !hosts.contains(&url.host_str().unwrap_or(""))
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(invalid());
        }
        url.path().trim_matches('/').to_string()
    } else {
        raw.to_string()
    };
    let valid = match provider {
        Provider::Github => {
            let parts: Vec<_> = resource.split('/').collect();
            parts.len() == 2
                && parts.iter().all(|part| {
                    !part.is_empty()
                        && part.len() <= 100
                        && part
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
                })
        }
        Provider::Vk => {
            resource.strip_prefix("id").is_some_and(positive_number)
                || resource
                    .strip_prefix("wall")
                    .and_then(|v| v.split_once('_'))
                    .is_some_and(|(owner, post)| {
                        owner.parse::<i64>().is_ok_and(|id| id != 0) && positive_number(post)
                    })
        }
        Provider::Telegram => {
            resource
                .parse::<i64>()
                .is_ok_and(|id| id != 0 && id.unsigned_abs() < (1_u64 << 53))
                || resource.strip_prefix('@').is_some_and(valid_username)
        }
        Provider::Instagram => positive_number(&resource),
    };
    if valid {
        Ok(resource)
    } else if provider == Provider::Telegram
        && raw.starts_with("https://t.me/")
        && valid_username(&resource)
    {
        Ok(format!("@{resource}"))
    } else {
        Err(invalid())
    }
}
fn positive_number(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|c| c.is_ascii_digit())
        && value.parse::<u64>().is_ok_and(|id| id > 0)
}
fn valid_username(value: &str) -> bool {
    (5..=64).contains(&value.len()) && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}
fn valid_id(value: &str) -> bool {
    (16..=80).contains(&value.len())
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-".contains(c))
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Checkpoint {
    phase: String,
    offset: u64,
    cursor: String,
    index: usize,
    queue: Vec<serde_json::Value>,
}
#[derive(Default)]
pub(crate) struct RawItem {
    pub id: String,
    pub author: Option<String>,
    pub author_label: Option<String>,
    pub text: String,
    pub kind: String,
    pub mode: String,
    pub url: Option<String>,
    pub created: Option<String>,
    pub reply_to: Option<String>,
    pub forwarded: bool,
    pub thread: Option<String>,
}
#[derive(Default)]
pub(crate) struct Page {
    pub items: Vec<RawItem>,
    pub done: bool,
    pub total: Option<u64>,
    pub not_analyzed: u64,
    pub unavailable: Vec<String>,
    pub truncated: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    grant: SourceGrant,
    documents: Vec<SourceDocument>,
    coverage: SourceCoverage,
    cursor: Option<String>,
    checkpoint: Checkpoint,
    steps: u32,
    started_at: Option<i64>,
    cancelled: bool,
    limitations: Vec<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportedRecord {
    provider: Provider,
    documents: Vec<SourceDocument>,
    warnings: Vec<String>,
}
struct SourceInner {
    directory: PathBuf,
    connections: Connections,
    store: SourceStore,
    gate: Mutex<()>,
    cancellations: Mutex<HashMap<String, Arc<AtomicBool>>>,
}
#[derive(Clone)]
pub struct Sources(Arc<SourceInner>);
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
    fn path(&self, id: &str) -> Result<PathBuf, String> {
        if !valid_id(id) {
            return Err("Неверный идентификатор разрешения.".into());
        }
        Ok(self.0.directory.join(format!("{id}.enc")))
    }
    fn imported(&self, resource: &str) -> Result<ImportedRecord, String> {
        let id = resource
            .strip_prefix("import:")
            .ok_or("Неверный импортированный ресурс.")?;
        self.0.store.read(&self.path(id)?)
    }
    fn create(&self, mut request: SourceGrantRequest) -> Result<SourceGrant, String> {
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
        if grant.request.author_id.is_some() {
            limitations.push("Ответы других участников исключены фильтром автора; контекст реплик может быть неполным.".into());
        }
        if grant.request.from.is_some() || grant.request.to.is_some() {
            limitations
                .push("Материал без подтверждённого времени исключён выбранным периодом.".into());
        }
        let record = Record {
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
        Ok(grant)
    }
    fn flag(&self, id: &str) -> Result<Arc<AtomicBool>, String> {
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
    fn execute(&self, id: &str, action: SourceAction) -> Result<SourceResult, String> {
        let path = self.path(id)?;
        let flag = self.flag(id)?;
        let _guard = self
            .0
            .gate
            .lock()
            .map_err(|_| "Ошибка состояния корпуса.")?;
        let mut record: Record = self.0.store.read(&path)?;
        let now = Utc::now().timestamp_millis();
        if record.grant.id != id {
            return Err("Корпус не соответствует разрешению.".into());
        }
        if record.cancelled || flag.load(Ordering::SeqCst) {
            return Err("Сбор отменён. Возобновите выбранное задание явно.".into());
        }
        if timestamp(&record.grant.expires_at)? * 1000 <= now {
            return Err("Разрешение истекло. Выберите источник заново.".into());
        }
        if !record.grant.request.allow_model_processing
            || !record
                .grant
                .request
                .operations
                .iter()
                .any(|operation| operation == action.operation())
        {
            return Err("Операция не разрешена для этого источника.".into());
        }
        if !record.grant.request.resource_id.starts_with("import:")
            && self
                .0
                .connections
                .account_id(record.grant.request.provider)?
                != record.grant.account_id
        {
            return Err("Подключённый аккаунт изменился. Выберите источник заново.".into());
        }
        let started = *record.started_at.get_or_insert(now);
        if record.steps >= record.grant.request.budget.max_steps
            || now.saturating_sub(started) as u64 >= record.grant.request.budget.max_duration_ms
        {
            record.coverage.truncated = true;
            record.coverage.complete = false;
            record.limitations.push(
                "Общий бюджет задания исчерпан; он не сбрасывается при возобновлении.".into(),
            );
            self.0.store.write(&path, &record)?;
            return Err("source-budget-exceeded".into());
        }
        record.steps += 1;
        // Persist the spent step before network I/O, including failed and repeated requests.
        self.0.store.write(&path, &record)?;
        let selected = match action {
            SourceAction::Describe | SourceAction::Coverage => vec![],
            SourceAction::Read { document_ids } => {
                if document_ids.len() > 20
                    || document_ids.is_empty()
                    || document_ids.iter().collect::<HashSet<_>>().len() != document_ids.len()
                {
                    return Err("Нужно выбрать от 1 до 20 разных документов.".into());
                }
                let mut result = Vec::new();
                for id in document_ids {
                    result.push(
                        record
                            .documents
                            .iter()
                            .find(|document| document.id == id && document.partition == "reference")
                            .cloned()
                            .ok_or("Документ не входит в разрешённую reference-выборку.")?,
                    );
                }
                result
            }
            SourceAction::Search { query, limit } => {
                if query.trim().is_empty() || query.len() > 256 || !(1..=20).contains(&limit) {
                    return Err("Некорректный поисковый запрос.".into());
                }
                let query = query.to_lowercase();
                record
                    .documents
                    .iter()
                    .filter(|document| {
                        document.partition == "reference"
                            && document.text.to_lowercase().contains(&query)
                    })
                    .take(limit)
                    .cloned()
                    .collect()
            }
            SourceAction::Collect { cursor, limit } => {
                if !(1..=20).contains(&limit) || cursor != record.cursor {
                    return Err("Некорректный размер порции или устаревший checkpoint.".into());
                }
                if record.coverage.complete {
                    vec![]
                } else {
                    let deadline =
                        started.saturating_add(record.grant.request.budget.max_duration_ms as i64);
                    let mut page = if record.grant.request.resource_id.starts_with("import:") {
                        let imported = self.imported(&record.grant.request.resource_id)?;
                        let end = (record.checkpoint.index + limit).min(imported.documents.len());
                        let items = imported.documents[record.checkpoint.index.min(end)..end]
                            .iter()
                            .map(|doc| RawItem {
                                id: doc.id.clone(),
                                author: doc.author_id.clone(),
                                author_label: doc.author_label.clone(),
                                text: doc.text.clone(),
                                kind: doc.kind.clone(),
                                mode: doc.mode.clone(),
                                url: doc.source_url.clone(),
                                created: doc.created_at.clone(),
                                reply_to: doc.reply_to.clone(),
                                forwarded: doc.is_forwarded,
                                thread: None,
                            })
                            .collect();
                        record.checkpoint.index = end;
                        record.limitations.extend(imported.warnings);
                        Page {
                            items,
                            done: end == imported.documents.len(),
                            total: Some(imported.documents.len() as u64),
                            ..Page::default()
                        }
                    } else {
                        readers::next_page(
                            &self.0.connections,
                            &record.grant.request,
                            &record.grant.account_id,
                            &mut record.checkpoint,
                            limit,
                            &flag,
                            deadline,
                        )?
                    };
                    if page.items.len() > limit {
                        return Err("Источник превысил разрешённый размер порции.".into());
                    }
                    if flag.load(Ordering::SeqCst) {
                        record.cancelled = true;
                        self.0.store.write(&path, &record)?;
                        return Err("Сбор отменён.".into());
                    }
                    if Utc::now().timestamp_millis() >= deadline {
                        record.coverage.truncated = true;
                        record.coverage.complete = false;
                        self.0.store.write(&path, &record)?;
                        return Err("source-budget-exceeded".into());
                    }
                    let mut result = Vec::new();
                    let mut budget_stopped = false;
                    let mut hashes: HashSet<String> = record
                        .documents
                        .iter()
                        .map(|doc| doc.content_hash.clone())
                        .collect();
                    let mut ids: HashSet<String> =
                        record.documents.iter().map(|doc| doc.id.clone()).collect();
                    for item in page.items {
                        if item.text.is_empty() {
                            record.coverage.not_analyzed += 1;
                            continue;
                        }
                        if !in_scope(&record.grant.request, &item) {
                            continue;
                        }
                        let doc = document(
                            &record.grant.request,
                            item,
                            !record.grant.request.resource_id.starts_with("import:"),
                        );
                        if ids.contains(&doc.id) || hashes.contains(&doc.content_hash) {
                            continue;
                        }
                        if record.coverage.documents
                            >= record.grant.request.budget.max_documents as u64
                            || record.coverage.bytes + doc.text.len() as u64
                                > record.grant.request.budget.max_bytes
                        {
                            budget_stopped = true;
                            page.truncated = true;
                            page.done = false;
                            page.unavailable.push("Достигнут лимит документов или байтов; оставшийся материал не прочитан.".into());
                            break;
                        }
                        record.coverage.documents += 1;
                        record.coverage.bytes += doc.text.len() as u64;
                        if doc.text.len() > 24000 {
                            page.truncated = true;
                            page.not_analyzed += 1;
                            page.unavailable.push("Длинные документы сохранены полностью локально, но модель получает первые 24000 байт; оставшийся текст не анализировался.".into());
                        }
                        ids.insert(doc.id.clone());
                        hashes.insert(doc.content_hash.clone());
                        if doc.partition == "reference" {
                            result.push(doc.clone());
                        }
                        record.documents.push(doc);
                    }
                    record.coverage.total_documents =
                        page.total.or(record.coverage.total_documents);
                    record.coverage.not_analyzed += page.not_analyzed;
                    record.coverage.unavailable.extend(page.unavailable);
                    record.coverage.unavailable.sort();
                    record.coverage.unavailable.dedup();
                    record.coverage.truncated |= page.truncated;
                    record.coverage.complete = page.done && !record.coverage.truncated;
                    record.cursor = if page.done || budget_stopped {
                        None
                    } else {
                        Some(random_id(18)?)
                    };
                    result
                }
            }
        };
        record.limitations.sort();
        record.limitations.dedup();
        self.0.store.write(&path, &record)?;
        let documents: Vec<_> = selected.into_iter().map(bounded_document).collect();
        Ok(SourceResult {
            snapshot_id: record.grant.snapshot_id,
            documents,
            coverage: record.coverage,
            cursor: record.cursor,
            limitations: record.limitations,
        })
    }
    fn cancel(&self, id: &str, cancelled: bool) -> Result<(), String> {
        self.path(id)?;
        self.flag(id)?.store(cancelled, Ordering::SeqCst);
        let _guard = self
            .0
            .gate
            .lock()
            .map_err(|_| "Ошибка состояния корпуса.")?;
        let path = self.path(id)?;
        let mut record: Record = self.0.store.read(&path)?;
        if record.grant.id != id {
            return Err("Неверный корпус.".into());
        }
        record.cancelled = cancelled;
        self.0.store.write(&path, &record)
    }
    fn reserve_model_step(&self, id: &str) -> Result<(), String> {
        let path = self.path(id)?;
        let _guard = self
            .0
            .gate
            .lock()
            .map_err(|_| "Ошибка состояния корпуса.")?;
        let mut record: Record = self.0.store.read(&path)?;
        let now = Utc::now().timestamp_millis();
        if record.grant.id != id || !record.grant.request.allow_model_processing {
            return Err("Разрешение корпуса недействительно.".into());
        }
        if record.cancelled || self.flag(id)?.load(Ordering::SeqCst) {
            return Err("Сбор отменён.".into());
        }
        if timestamp(&record.grant.expires_at)? * 1000 <= now {
            return Err("Разрешение истекло.".into());
        }
        if !record.grant.request.resource_id.starts_with("import:")
            && self
                .0
                .connections
                .account_id(record.grant.request.provider)?
                != record.grant.account_id
        {
            return Err("Подключённый аккаунт изменился.".into());
        }
        let started = *record.started_at.get_or_insert(now);
        if record.steps >= record.grant.request.budget.max_steps
            || now.saturating_sub(started) as u64 >= record.grant.request.budget.max_duration_ms
        {
            return Err("source-budget-exceeded".into());
        }
        record.steps += 1;
        self.0.store.write(&path, &record)
    }
    fn clear(&self, provider: Provider) -> Result<(), String> {
        // Serialize enumeration with creation/import as well as deletion. Otherwise an
        // import finishing between scan and lock could survive a successful clear.
        let _guard = self
            .0
            .gate
            .lock()
            .map_err(|_| "Ошибка состояния корпуса.")?;
        let mut ids = HashSet::new();
        for directory in [&self.0.directory, &self.0.directory.join("v2")] {
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
    fn delete(&self, id: &str) -> Result<(), String> {
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
    fn import(&self, provider: Provider, path: &Path) -> Result<ImportedSource, String> {
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
fn remove_file(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => crate::connections::storage::sync_directory(
            path.parent().ok_or("Неверный путь корпуса.")?,
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("Не удалось удалить выбранный корпус.".into()),
    }
}
fn in_scope(request: &SourceGrantRequest, item: &RawItem) -> bool {
    if request
        .author_id
        .as_ref()
        .is_some_and(|author| item.author.as_ref() != Some(author))
    {
        return false;
    }
    if request.from.is_some() || request.to.is_some() {
        let Some(created) = item
            .created
            .as_deref()
            .and_then(|date| timestamp(date).ok())
        else {
            return false;
        };
        if request
            .from
            .as_deref()
            .and_then(|v| timestamp(v).ok())
            .is_some_and(|from| created < from)
            || request
                .to
                .as_deref()
                .and_then(|v| timestamp(v).ok())
                .is_some_and(|to| created > to)
        {
            return false;
        }
    }
    true
}
fn document(request: &SourceGrantRequest, item: RawItem, retrieved: bool) -> SourceDocument {
    let content_hash = format!("{:x}", Sha256::digest(item.text.as_bytes()));
    let id = opaque_id(request, &item.id);
    let authorship = match (&request.author_id, &item.author) {
        (Some(expected), Some(actual)) if expected == actual => "owner",
        (_, Some(_)) => "other",
        _ => "unknown",
    };
    let partition = if retrieved
        && request.provider == Provider::Github
        && item
            .thread
            .as_ref()
            .is_some_and(|thread| Sha256::digest(thread.as_bytes())[0] % 5 == 0)
    {
        "holdout"
    } else {
        "reference"
    };
    SourceDocument {
        id,
        provider: request.provider,
        resource_id: request.resource_id.clone(),
        author_id: item.author,
        author_label: item.author_label,
        authorship: authorship.into(),
        kind: if item.kind.is_empty() {
            "document".into()
        } else {
            item.kind
        },
        mode: if item.mode.is_empty() {
            "formal".into()
        } else {
            item.mode
        },
        text: item.text,
        source_url: item.url,
        created_at: item
            .created
            .as_deref()
            .and_then(|date| timestamp(date).ok())
            .and_then(iso),
        retrieved_at: if retrieved {
            Some(Utc::now().to_rfc3339())
        } else {
            None
        },
        content_hash,
        reply_to: item.reply_to.map(|parent| opaque_id(request, &parent)),
        is_forwarded: item.forwarded,
        truncated: false,
        partition: partition.into(),
    }
}
fn opaque_id(request: &SourceGrantRequest, raw: &str) -> String {
    format!(
        "d{:x}",
        Sha256::digest(
            format!(
                "{}\n{}\n{}",
                request.provider.key(),
                request.resource_id,
                raw
            )
            .as_bytes()
        )
    )
}
fn bounded_document(mut doc: SourceDocument) -> SourceDocument {
    if doc.text.len() > 24000 {
        let mut end = 24000;
        while !doc.text.is_char_boundary(end) {
            end -= 1;
        }
        doc.text.truncate(end);
        doc.truncated = true;
    }
    doc
}

#[tauri::command]
pub async fn create_source_grant(
    request: SourceGrantRequest,
    sources: tauri::State<'_, Sources>,
) -> Result<SourceGrant, String> {
    let sources = sources.inner().clone();
    tauri::async_runtime::spawn_blocking(move || sources.create(request))
        .await
        .map_err(|_| "Не удалось создать разрешение источника.".to_string())?
}
#[tauri::command]
pub async fn source_action(
    grant_id: String,
    action: SourceAction,
    sources: tauri::State<'_, Sources>,
) -> Result<SourceResult, String> {
    let sources = sources.inner().clone();
    tauri::async_runtime::spawn_blocking(move || sources.execute(&grant_id, action))
        .await
        .map_err(|_| "Не удалось выполнить действие источника.".to_string())?
}
#[tauri::command]
pub async fn cancel_source(
    grant_id: String,
    sources: tauri::State<'_, Sources>,
) -> Result<(), String> {
    let sources = sources.inner().clone();
    sources.flag(&grant_id)?.store(true, Ordering::SeqCst);
    tauri::async_runtime::spawn_blocking(move || sources.cancel(&grant_id, true))
        .await
        .map_err(|_| "Не удалось отменить сбор.".to_string())?
}
#[tauri::command]
pub async fn resume_source(
    grant_id: String,
    sources: tauri::State<'_, Sources>,
) -> Result<(), String> {
    let sources = sources.inner().clone();
    tauri::async_runtime::spawn_blocking(move || sources.cancel(&grant_id, false))
        .await
        .map_err(|_| "Не удалось возобновить сбор.".to_string())?
}
#[tauri::command]
pub async fn reserve_model_step(
    grant_id: String,
    sources: tauri::State<'_, Sources>,
) -> Result<(), String> {
    let sources = sources.inner().clone();
    tauri::async_runtime::spawn_blocking(move || sources.reserve_model_step(&grant_id))
        .await
        .map_err(|_| "Не удалось зарезервировать шаг модели.".to_string())?
}
#[tauri::command]
pub async fn clear_source_data(
    provider: Provider,
    sources: tauri::State<'_, Sources>,
) -> Result<(), String> {
    let sources = sources.inner().clone();
    tauri::async_runtime::spawn_blocking(move || sources.clear(provider))
        .await
        .map_err(|_| "Не удалось удалить данные источника.".to_string())?
}
#[tauri::command]
pub async fn delete_source(
    grant_id: String,
    sources: tauri::State<'_, Sources>,
) -> Result<(), String> {
    let sources = sources.inner().clone();
    tauri::async_runtime::spawn_blocking(move || sources.delete(&grant_id))
        .await
        .map_err(|_| "Не удалось удалить выбранный корпус.".to_string())?
}
#[tauri::command]
pub async fn import_source(
    app: tauri::AppHandle,
    provider: Provider,
    sources: tauri::State<'_, Sources>,
) -> Result<Option<ImportedSource>, String> {
    let sources = sources.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(file) = app
            .dialog()
            .file()
            .add_filter(
                "Экспорт сообщений или выбранный текст",
                &["json", "txt", "md"],
            )
            .blocking_pick_file()
        else {
            return Ok(None);
        };
        sources
            .import(
                provider,
                &file.into_path().map_err(|_| "Нужен локальный файл.")?,
            )
            .map(Some)
    })
    .await
    .map_err(|_| "Не удалось импортировать источник.".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> SourceGrantRequest {
        SourceGrantRequest {
            provider: Provider::Github,
            resource_id: "https://github.com/owner/repository".into(),
            author_id: Some("author".into()),
            from: Some("2024-01-01".into()),
            to: Some("2025-01-01".into()),
            allow_model_processing: true,
            operations: vec!["collect".into(), "read".into()],
            budget: SourceBudget {
                max_steps: 16,
                max_documents: 1000,
                max_bytes: 1048576,
                max_duration_ms: 300000,
            },
        }
    }
    #[test]
    fn native_grant_canonicalizes_selected_resource_and_rejects_scope_escalation() {
        let mut valid = request();
        validate_request(&mut valid).unwrap();
        assert_eq!(valid.resource_id, "owner/repository");
        for resource in [
            "https://github.com.evil/owner/repo",
            "https://user:pass@github.com/owner/repo",
            "https://github.com/owner/repo?next=other",
            "../../secrets",
            "owner/repo/../../other",
            "https://127.0.0.1/repo",
        ] {
            let mut invalid = request();
            invalid.resource_id = resource.into();
            assert!(validate_request(&mut invalid).is_err());
        }
    }
    #[test]
    fn grants_require_consent_bounded_budgets_and_ordered_dates() {
        let mut invalid = request();
        invalid.allow_model_processing = false;
        assert!(validate_request(&mut invalid).is_err());
        let mut invalid = request();
        invalid.budget.max_documents = 10000000;
        assert!(validate_request(&mut invalid).is_err());
        let mut invalid = request();
        invalid.operations.push("send-message".into());
        assert!(validate_request(&mut invalid).is_err());
        let mut invalid = request();
        invalid.from = Some("2026-01-01".into());
        assert!(validate_request(&mut invalid).is_err());
    }
    struct NoNetwork;
    impl crate::connections::http::Http for NoNetwork {
        fn get(
            &self,
            _: &str,
            _: &[(&str, String)],
            _: Option<&str>,
        ) -> Result<serde_json::Value, String> {
            panic!("local source must never access network")
        }
        fn post(&self, _: &str, _: &[(&str, String)]) -> Result<serde_json::Value, String> {
            panic!("local source must never access network")
        }
    }
    fn local() -> (Sources, PathBuf) {
        let directory =
            std::env::temp_dir().join(format!("proteus-source-test-{}", random_id(18).unwrap()));
        let hub = Connections::test(
            Arc::new(crate::connections::storage::MemorySecrets::default()),
            Arc::new(NoNetwork),
        );
        (Sources::new(directory.clone(), hub), directory)
    }
    fn local_grant(sources: &Sources, provider: Provider, max_steps: u32) -> SourceGrant {
        let id = random_id(18).unwrap();
        let mut request = request();
        request.provider = provider;
        request.resource_id = format!("import:{id}");
        request.author_id = None;
        request.from = None;
        request.to = None;
        request.budget.max_steps = max_steps;
        let doc = document(
            &request,
            RawItem {
                id: "one".into(),
                text: "  exact\ntext".into(),
                ..RawItem::default()
            },
            false,
        );
        sources
            .0
            .store
            .write(
                &sources.path(&id).unwrap(),
                &ImportedRecord {
                    provider,
                    documents: vec![doc],
                    warnings: vec![],
                },
            )
            .unwrap();
        sources.create(request).unwrap()
    }
    #[test]
    fn import_collection_preserves_undated_text_and_resume_does_not_reset_shared_budget() {
        let (sources, directory) = local();
        let grant = local_grant(&sources, Provider::Instagram, 2);
        sources.reserve_model_step(&grant.id).unwrap();
        let page = sources
            .execute(
                &grant.id,
                SourceAction::Collect {
                    cursor: None,
                    limit: 20,
                },
            )
            .unwrap();
        assert_eq!(page.documents[0].text, "  exact\ntext");
        assert!(page.coverage.complete);
        assert!(page.cursor.is_none());
        sources.cancel(&grant.id, true).unwrap();
        sources.cancel(&grant.id, false).unwrap();
        assert_eq!(
            sources.reserve_model_step(&grant.id).unwrap_err(),
            "source-budget-exceeded"
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn interrupted_selected_delete_can_be_retried_without_reactivating_old_data() {
        let (sources, directory) = local();
        let grant = local_grant(&sources, Provider::Telegram, 10);
        let legacy = sources.path(&grant.id).unwrap();
        std::fs::create_dir(&legacy).unwrap();
        assert!(sources.delete(&grant.id).is_err());
        assert!(sources.execute(&grant.id, SourceAction::Coverage).is_err());
        std::fs::remove_dir(&legacy).unwrap();
        assert!(sources.delete(&grant.id).is_ok());
        assert!(!storage_codec::current_path(&legacy).unwrap().exists());
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn deletion_only_removes_selected_provider_owned_ciphertext() {
        let (sources, directory) = local();
        let tg = local_grant(&sources, Provider::Telegram, 10);
        let ig = local_grant(&sources, Provider::Instagram, 10);
        std::fs::write(directory.join("unrelated.txt"), "keep").unwrap();
        sources.clear(Provider::Telegram).unwrap();
        assert!(!sources.path(&tg.id).unwrap().exists());
        assert!(!sources
            .path(tg.request.resource_id.trim_start_matches("import:"))
            .unwrap()
            .exists());
        assert!(storage_codec::current_path(&sources.path(&ig.id).unwrap())
            .unwrap()
            .exists());
        assert!(directory.join("unrelated.txt").exists());
        assert!(sources.delete("../../other").is_err());
        sources.delete(&ig.id).unwrap();
        assert!(!sources.path(&ig.id).unwrap().exists());
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn opaque_reply_ids_remain_linked_after_import_and_collection() {
        let mut selected = request();
        selected.resource_id = "import:abcdefghijklmnop".into();
        let parent = document(
            &selected,
            RawItem {
                id: "chat:1".into(),
                text: "parent".into(),
                ..RawItem::default()
            },
            false,
        );
        let child = document(
            &selected,
            RawItem {
                id: "chat:2".into(),
                text: "child".into(),
                reply_to: Some("chat:1".into()),
                ..RawItem::default()
            },
            false,
        );
        assert_eq!(child.reply_to.as_deref(), Some(parent.id.as_str()));
        let collected_parent = document(
            &selected,
            RawItem {
                id: parent.id,
                text: parent.text,
                ..RawItem::default()
            },
            false,
        );
        let collected_child = document(
            &selected,
            RawItem {
                id: child.id,
                text: child.text,
                reply_to: child.reply_to,
                ..RawItem::default()
            },
            false,
        );
        assert_eq!(collected_child.reply_to, Some(collected_parent.id));
    }
    #[test]
    fn provider_iso_offsets_are_normalized_before_time_scope_checks() {
        assert_eq!(
            timestamp("2024-01-01T00:00:00+0000").unwrap(),
            timestamp("2024-01-01T00:00:00Z").unwrap()
        );
        let mut selected = request();
        selected.from = Some("2024-01-01T00:00:00Z".into());
        selected.to = None;
        selected.author_id = None;
        let item = RawItem {
            id: "1".into(),
            created: Some("2024-01-01T03:00:00+0300".into()),
            ..RawItem::default()
        };
        assert!(in_scope(&selected, &item));
    }
}

#[cfg(test)]
mod wire_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn source_read_accepts_canonical_document_ids() {
        let action =
            serde_json::from_value::<SourceAction>(json!({"action":"read","documentIds":["d:1"]}));
        assert!(
            matches!(action, Ok(SourceAction::Read { document_ids }) if document_ids == ["d:1"])
        );
    }
    #[test]
    fn source_read_rejects_legacy_wire() {
        assert!(serde_json::from_value::<SourceAction>(
            json!({"action":"read","document-ids":["d:1"]})
        )
        .is_err());
    }
    #[test]
    fn source_budget_emits_exact_canonical_keys() {
        let value = serde_json::to_value(SourceBudget {
            max_steps: 1,
            max_documents: 2,
            max_bytes: 1024,
            max_duration_ms: 1000,
        })
        .unwrap();
        assert_eq!(
            value,
            json!({"maxSteps":1,"maxDocuments":2,"maxBytes":1024,"maxDurationMs":1000})
        );
    }
    #[test]
    fn source_document_keeps_null_presence_and_exact_text() {
        let text = "\r\n  Чужой: é 😀\t";
        let value = serde_json::to_value(SourceDocument {
            id: "d1".into(),
            provider: Provider::Telegram,
            resource_id: "chat:1".into(),
            author_id: None,
            author_label: None,
            authorship: "unknown".into(),
            kind: "message".into(),
            mode: "dialogue".into(),
            text: text.into(),
            source_url: None,
            created_at: None,
            retrieved_at: None,
            content_hash: "h".into(),
            reply_to: None,
            is_forwarded: true,
            truncated: false,
            partition: "training".into(),
        })
        .unwrap();
        assert_eq!(
            value,
            json!({"id":"d1","provider":"telegram","resourceId":"chat:1","authorId":null,"authorLabel":null,
            "authorship":"unknown","kind":"message","mode":"dialogue","text":text,"sourceUrl":null,"createdAt":null,"retrievedAt":null,
            "contentHash":"h","replyTo":null,"isForwarded":true,"truncated":false,"partition":"training"})
        );
    }
    #[test]
    fn matches_shared_frontend_wire_fixture() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../src/shared/lib/fixtures/native-wire.json"
        ))
        .unwrap();
        let value: SourceGrantRequest =
            serde_json::from_value(fixture["sourceRequest"].clone()).unwrap();
        assert_eq!(
            serde_json::to_value(value).unwrap(),
            fixture["sourceRequest"]
        );
        let value: SourceGrant = serde_json::from_value(fixture["sourceGrant"].clone()).unwrap();
        assert_eq!(serde_json::to_value(value).unwrap(), fixture["sourceGrant"]);
        let value: SourceResult = serde_json::from_value(fixture["sourceResult"].clone()).unwrap();
        assert_eq!(
            serde_json::to_value(value).unwrap(),
            fixture["sourceResult"]
        );
        let value: SourceAction = serde_json::from_value(fixture["sourceAction"].clone()).unwrap();
        assert_eq!(
            serde_json::to_value(value).unwrap(),
            fixture["sourceAction"]
        );
        let value: ImportedSource =
            serde_json::from_value(fixture["importedSource"].clone()).unwrap();
        assert_eq!(
            serde_json::to_value(value).unwrap(),
            fixture["importedSource"]
        );
    }
}
