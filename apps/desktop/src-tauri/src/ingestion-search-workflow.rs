//! Versioned local-search boundary; native jobs remain the source of authority.
use super::operations::Operation;
use crate::corpus_database::search::{SearchIndexReceipt, SearchResponse};
use crate::sources::Sources;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IndexRequest {
    pub schema_version: u8,
    pub operation_id: String,
    pub job_id: String,
    pub snapshot_id: String,
    pub max_duration_ms: i64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueryPeriod {
    #[serde(deserialize_with = "required_bound")]
    pub from: Option<i64>,
    #[serde(deserialize_with = "required_bound")]
    pub to: Option<i64>,
    pub include_undated: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueryRequest {
    pub schema_version: u8,
    pub operation_id: String,
    pub job_id: String,
    pub snapshot_id: String,
    pub persona_id: String,
    pub query_text: String,
    pub period: QueryPeriod,
    pub limit: i64,
    pub offset: i64,
    #[serde(deserialize_with = "required_bound")]
    pub expected_index_generation: Option<i64>,
    pub max_duration_ms: i64,
}
fn required_bound<'de, D: serde::Deserializer<'de>>(value: D) -> Result<Option<i64>, D::Error> {
    Option::<i64>::deserialize(value)
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexResult {
    schema_version: u8,
    #[serde(flatten)]
    pub index: SearchIndexReceipt,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryResult {
    schema_version: u8,
    #[serde(flatten)]
    pub result: SearchResponse,
}

fn valid_id(value: &str, limit: usize) -> bool {
    !value.trim().is_empty() && value.len() <= limit && !value.chars().any(char::is_control)
}
fn validate_identity(
    version: u8,
    operation_id: &str,
    job_id: &str,
    snapshot_id: &str,
) -> Result<(), String> {
    if version != 1
        || !valid_id(operation_id, 128)
        || !valid_id(job_id, 128)
        || !valid_id(snapshot_id, 256)
    {
        return Err("Неверные идентификаторы запроса поиска.".into());
    }
    Ok(())
}
impl IndexRequest {
    pub fn validate(&self) -> Result<(), String> {
        validate_identity(
            self.schema_version,
            &self.operation_id,
            &self.job_id,
            &self.snapshot_id,
        )?;
        if !(1..=3_600_000).contains(&self.max_duration_ms) {
            return Err("Неверный бюджет индексирования.".into());
        }
        Ok(())
    }
}
impl QueryRequest {
    pub fn validate(&self) -> Result<(), String> {
        validate_identity(
            self.schema_version,
            &self.operation_id,
            &self.job_id,
            &self.snapshot_id,
        )?;
        self.input().validate()
    }
    fn input(&self) -> super::search_query::QueryInput<'_> {
        super::search_query::QueryInput {
            job_id: &self.job_id,
            snapshot_id: &self.snapshot_id,
            persona_id: &self.persona_id,
            query_text: &self.query_text,
            period: crate::corpus_database::search::SearchPeriod {
                from: self.period.from,
                to: self.period.to,
                include_undated: self.period.include_undated,
            },
            limit: self.limit,
            offset: self.offset,
            expected_index_generation: self.expected_index_generation,
            max_duration_ms: self.max_duration_ms,
        }
    }
}
fn check_operation(operation: &Operation, expected: &str) -> Result<(), String> {
    if operation.id() != expected {
        return Err("Запрос относится к другой операции поиска.".into());
    }
    operation.check()
}
pub fn rebuild(
    sources: &Sources,
    operation: &Operation,
    request: &IndexRequest,
    now_ms: impl Fn() -> i64,
) -> Result<IndexResult, String> {
    request.validate()?;
    check_operation(operation, &request.operation_id)?;
    let index = super::search::rebuild(
        sources,
        operation,
        &super::search::IndexInput {
            job_id: &request.job_id,
            snapshot_id: &request.snapshot_id,
            max_duration_ms: request.max_duration_ms,
        },
        now_ms,
    )?;
    check_operation(operation, &request.operation_id)?;
    Ok(IndexResult {
        schema_version: 1,
        index,
    })
}
pub fn query(
    sources: &Sources,
    operation: &Operation,
    request: &QueryRequest,
    now_ms: impl Fn() -> i64,
) -> Result<QueryResult, String> {
    request.validate()?;
    check_operation(operation, &request.operation_id)?;
    let result = super::search_query::query(sources, operation, &request.input(), now_ms)?;
    check_operation(operation, &request.operation_id)?;
    Ok(QueryResult {
        schema_version: 1,
        result,
    })
}
