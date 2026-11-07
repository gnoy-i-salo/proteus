//! Bounded preview metadata shared by acquisition adapters. It never holds bodies
//! or grants authority; native selection/permission checks happen after preview.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreviewResource {
    pub id: String,
    pub label: String,
    pub author_ids: Vec<String>,
    pub estimated_documents: Option<u64>,
}
struct Resource {
    id: String,
    label: String,
    authors: BTreeSet<String>,
    documents: u64,
}
pub struct PreviewCollector {
    resources: Vec<Resource>,
    indexes: HashMap<String, usize>,
    max_documents: u64,
    documents: u64,
    max_metadata_bytes: usize,
    metadata_bytes: usize,
    failed: bool,
}
impl PreviewCollector {
    pub fn new(max_documents: u64, max_metadata_bytes: usize) -> Result<Self, String> {
        if !(1..=9_007_199_254_740_991).contains(&max_documents)
            || !(1..=32 * 1024 * 1024).contains(&max_metadata_bytes)
        {
            return Err("Неверный бюджет предпросмотра.".into());
        }
        Ok(Self {
            resources: vec![],
            indexes: HashMap::new(),
            max_documents,
            documents: 0,
            max_metadata_bytes,
            metadata_bytes: 0,
            failed: false,
        })
    }
    fn reserve(&mut self, bytes: usize) -> Result<(), String> {
        let next = self
            .metadata_bytes
            .checked_add(bytes)
            .filter(|next| *next <= self.max_metadata_bytes)
            .ok_or("Предпросмотр превышает бюджет метаданных. Выберите меньший архив.")?;
        self.metadata_bytes = next;
        Ok(())
    }
    pub fn resource(&mut self, id: &str, label: &str) -> Result<(), String> {
        let result = self.add_resource(id, label);
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    fn add_resource(&mut self, id: &str, label: &str) -> Result<(), String> {
        if self.failed || id.trim().is_empty() || self.indexes.contains_key(id) {
            return Err("Неоднозначный или неверный источник предпросмотра.".into());
        }
        if self.resources.len() >= 512 {
            return Err("В одном предпросмотре поддерживается до 512 источников.".into());
        }
        // Include both copies of the ID and conservative map/vector overhead.
        let cost = id
            .len()
            .checked_mul(2)
            .and_then(|n| n.checked_add(label.len()))
            .and_then(|n| n.checked_add(256))
            .ok_or("Слишком большие метаданные.")?;
        self.reserve(cost)?;
        self.indexes.insert(id.into(), self.resources.len());
        self.resources.push(Resource {
            id: id.into(),
            label: label.into(),
            authors: BTreeSet::new(),
            documents: 0,
        });
        Ok(())
    }
    pub fn document(&mut self, resource_id: &str, author_id: Option<&str>) -> Result<(), String> {
        let result = self.add_document(resource_id, author_id);
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    fn add_document(&mut self, resource_id: &str, author_id: Option<&str>) -> Result<(), String> {
        if self.failed || self.documents >= self.max_documents {
            return Err(
                "Предпросмотр превышает бюджет документов или уже завершился ошибкой.".into(),
            );
        }
        let index = *self
            .indexes
            .get(resource_id)
            .ok_or("Документ не принадлежит источнику предпросмотра.")?;
        if let Some(id) = author_id {
            if id.len() > 128
                || id.chars().any(char::is_control)
                || id
                    .trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
                    .is_empty()
            {
                return Err("Неверный идентификатор автора источника.".into());
            }
            if !self.resources[index].authors.contains(id) {
                if self.resources[index].authors.len() >= 4096 {
                    return Err(
                        "В одном источнике предпросмотра поддерживается до 4096 авторов.".into(),
                    );
                }
                self.reserve(id.len() + 128)?;
                self.resources[index].authors.insert(id.into());
            }
        }
        self.documents += 1;
        self.resources[index].documents += 1;
        Ok(())
    }
    pub fn finish(self) -> Result<Vec<PreviewResource>, String> {
        if self.failed {
            return Err("Нельзя использовать неполный предпросмотр после ошибки.".into());
        }
        Ok(self
            .resources
            .into_iter()
            .map(|r| PreviewResource {
                id: r.id,
                label: r.label,
                author_ids: r.authors.into_iter().collect(),
                estimated_documents: Some(r.documents),
            })
            .collect())
    }
}
#[cfg(test)]
#[path = "ingestion-preview-tests.rs"]
mod tests;
