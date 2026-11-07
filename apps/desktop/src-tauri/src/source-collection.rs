use super::binding_policy;
use super::{
    corpus::Sources,
    document::{bounded_document, document, in_scope},
    grant::timestamp,
    model::{Page, RawItem, Record, SourceAction, SourceDocument, SourceResult},
    readers,
};
use crate::connections::storage::random_id;
use chrono::Utc;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};
impl Sources {
    pub(super) fn execute(&self, id: &str, action: SourceAction) -> Result<SourceResult, String> {
        self.execute_scoped(id, None, action)
    }
    pub(super) fn execute_scoped(
        &self,
        id: &str,
        binding_id: Option<&str>,
        action: SourceAction,
    ) -> Result<SourceResult, String> {
        let path = self.path(id)?;
        let flag = self.flag(id)?;
        let _guard = self
            .0
            .gate
            .lock()
            .map_err(|_| "Ошибка состояния корпуса.")?;
        let mut record: Record = self.0.store.read(&path)?;
        let now = Utc::now().timestamp_millis();
        match (&record.binding, binding_id) {
            (None, None) => {}
            (Some(binding), Some(selected)) if binding.id == selected => {
                binding_policy::validate_authority(binding, &record.grant, now)?
            }
            _ => return Err("Используйте выбранную нативную привязку источника.".into()),
        }
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
                self.collect(&mut record, &flag, started, cursor, limit, &path)?
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
    pub(super) fn cancel(&self, id: &str, cancelled: bool) -> Result<(), String> {
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
    pub(super) fn reserve_model_step(&self, id: &str) -> Result<(), String> {
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
    fn collect(
        &self,
        record: &mut Record,
        flag: &AtomicBool,
        started: i64,
        cursor: Option<String>,
        limit: usize,
        path: &Path,
    ) -> Result<Vec<SourceDocument>, String> {
        if !(1..=20).contains(&limit) || cursor != record.cursor {
            return Err("Некорректный размер порции или устаревший checkpoint.".into());
        }
        Ok(if record.coverage.complete {
            vec![]
        } else {
            let deadline =
                started.saturating_add(record.grant.request.budget.max_duration_ms as i64);
            let mut imported_originals = HashMap::new();
            let mut page = if record.grant.request.resource_id.starts_with("import:") {
                let imported = self.imported(&record.grant.request.resource_id)?;
                let end = (record.checkpoint.index + limit).min(imported.documents.len());
                if record.binding.is_some() {
                    imported_originals.extend(
                        imported.documents[record.checkpoint.index.min(end)..end]
                            .iter()
                            .map(|doc| (doc.id.clone(), doc.clone())),
                    );
                }
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
                    flag,
                    deadline,
                )?
            };
            if page.items.len() > limit {
                return Err("Источник превысил разрешённый размер порции.".into());
            }
            if flag.load(Ordering::SeqCst) {
                record.cancelled = true;
                self.0.store.write(path, record)?;
                return Err("Сбор отменён.".into());
            }
            if Utc::now().timestamp_millis() >= deadline {
                record.coverage.truncated = true;
                record.coverage.complete = false;
                self.0.store.write(path, record)?;
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
                let selected = if let Some(binding) = &record.binding {
                    binding_policy::speaker(binding, item.author.as_deref()).is_ok()
                        && binding_policy::in_period(binding, item.created.as_deref()).is_ok()
                } else {
                    in_scope(&record.grant.request, &item)
                };
                if !selected {
                    continue;
                }
                let doc = if let Some(original) = imported_originals.remove(&item.id) {
                    // Binding projection never rewrites saved provenance, authorship,
                    // IDs or reply relationships, including legacy unknown metadata.
                    original
                } else {
                    document(
                        &record.grant.request,
                        item,
                        !record.grant.request.resource_id.starts_with("import:"),
                    )
                };
                if ids.contains(&doc.id)
                    || (record.binding.is_none() && hashes.contains(&doc.content_hash))
                {
                    continue;
                }
                if record.coverage.documents >= record.grant.request.budget.max_documents as u64
                    || record.coverage.bytes + doc.text.len() as u64
                        > record.grant.request.budget.max_bytes
                {
                    budget_stopped = true;
                    page.truncated = true;
                    page.done = false;
                    page.unavailable.push(
                        "Достигнут лимит документов или байтов; оставшийся материал не прочитан."
                            .into(),
                    );
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
            record.coverage.total_documents = page.total.or(record.coverage.total_documents);
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
        })
    }
}
