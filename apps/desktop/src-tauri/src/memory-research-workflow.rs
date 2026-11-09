//! Host-only research admission; model actions cannot open or rebind a session.
use super::session::MemorySession;
use super::{
    registry::MemorySessions,
    types::{ModelCallBudget, RecallBudget},
};
use crate::local_embedding::service::LocalModelService;
use crate::{
    ingestion::search_workflow::QueryPeriod, local_embedding::model::EmbeddingModel,
    sources::Sources,
};
use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering;

#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ResearchMode {
    Lexical,
    Hybrid,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResearchModel {
    pub provider: String,
    pub model: String,
    pub effort: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenResearchRequest {
    pub schema_version: u8,
    pub operation_id: String,
    pub job_id: String,
    pub snapshot_id: String,
    pub persona_id: String,
    pub mode: ResearchMode,
    pub period: QueryPeriod,
    pub recall_budget: RecallBudget,
    pub model_budget: ModelCallBudget,
    pub model: ResearchModel,
}
impl OpenResearchRequest {
    pub fn validate(&self) -> Result<(), String> {
        let id = |value: &str, cap| {
            !value.trim().is_empty() && value.len() <= cap && !value.chars().any(char::is_control)
        };
        if self.schema_version != 1
            || !id(&self.operation_id, 128)
            || !id(&self.job_id, 128)
            || !id(&self.snapshot_id, 256)
            || !id(&self.persona_id, 256)
            || self
                .period
                .from
                .zip(self.period.to)
                .is_some_and(|(from, to)| from > to)
            || [self.period.from, self.period.to]
                .into_iter()
                .flatten()
                .any(|v| !(-9_007_199_254_740_991..=9_007_199_254_740_991).contains(&v))
        {
            return Err("Неверные параметры исследовательской сессии.".into());
        }
        self.recall_budget.validate()?;
        self.model_budget.validate()
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchSession {
    pub schema_version: u8,
    pub id: String,
    pub persona_id: String,
    pub corpus_snapshot_id: String,
    pub policy_version: String,
    pub mode: &'static str,
    pub index_generation: i64,
    pub expires_at: String,
}
pub fn open(
    sources: &Sources,
    sessions: &MemorySessions,
    request: &OpenResearchRequest,
    model: Option<&dyn EmbeddingModel>,
    now: impl Fn() -> i64,
) -> Result<ResearchSession, String> {
    let mut session = prepare(sources, request, &now)?;
    configure(&mut session, request.mode, model, &now)?;
    admit(session, sessions, now)
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FabricSession {
    #[serde(flatten)]
    pub session: ResearchSession,
    pub purpose: &'static str,
}
pub fn open_fabric(
    sources: &Sources,
    sessions: &MemorySessions,
    request: &OpenResearchRequest,
    now: impl Fn() -> i64,
) -> Result<FabricSession, String> {
    // Fabric reads pinned reference episodes; it does not issue semantic queries.
    if request.mode != ResearchMode::Lexical {
        return Err("Сессия Fabric читает опубликованную память без гибридного поиска.".into());
    }
    let mut session = bind_source(sources, request, &now)?;
    session.configure_fabric_model(
        model_binding(request, "generation"),
        request.model_budget.clone(),
        &now,
    )?;
    Ok(FabricSession {
        session: admit(session, sessions, now)?,
        purpose: "fabric",
    })
}
pub fn embedding_budget() -> crate::local_embedding::runtime::EmbeddingBudget {
    crate::local_embedding::runtime::EmbeddingBudget {
        max_batch_items: 1,
        max_batch_tokens: 512,
        max_resident_bytes: crate::local_embedding::runtime::RESIDENT_RESERVATION_BYTES,
        max_duration_ms: 60000,
    }
}
pub fn prepare(
    sources: &Sources,
    request: &OpenResearchRequest,
    now: impl Fn() -> i64,
) -> Result<MemorySession, String> {
    let mut session = bind_source(sources, request, &now)?;
    session.configure_research_model(
        model_binding(request, "corpus-analysis"),
        request.model_budget.clone(),
        now,
    )?;
    Ok(session)
}
fn model_binding(
    request: &OpenResearchRequest,
    operation: &str,
) -> super::session::InvocationBinding {
    super::session::InvocationBinding {
        provider: request.model.provider.clone(),
        model: request.model.model.clone(),
        effort: request.model.effort.clone(),
        operation: operation.into(),
    }
}
fn bind_source(
    sources: &Sources,
    request: &OpenResearchRequest,
    now: impl Fn() -> i64,
) -> Result<super::session::MemorySession, String> {
    use super::{
        budget::RecallLimits,
        session::{MemorySession, SessionInput},
    };
    request.validate()?;
    let provider = crate::ingestion::job_workflow::job_provider(sources, &request.job_id)?;
    let operation = sources.begin_ingestion_operation(&request.operation_id, provider)?;
    let budget = &request.recall_budget;
    MemorySession::bind(
        sources.clone(),
        operation,
        SessionInput {
            job_id: &request.job_id,
            snapshot_id: &request.snapshot_id,
            persona_id: &request.persona_id,
            period: crate::corpus_database::search::SearchPeriod {
                from: request.period.from,
                to: request.period.to,
                include_undated: request.period.include_undated,
            },
            limits: RecallLimits {
                max_calls: budget.max_calls as usize,
                max_candidates: budget.max_candidates as usize,
                max_episodes: budget.max_episodes as usize,
                max_source_bytes: budget.max_source_bytes as usize,
                max_context_tokens: budget.max_context_tokens as usize,
                max_duration_ms: budget.max_duration_ms as i64,
            },
        },
        &now,
    )
}
pub fn configure(
    session: &mut super::session::MemorySession,
    mode: ResearchMode,
    model: Option<&dyn EmbeddingModel>,
    now: impl Fn() -> i64,
) -> Result<(), String> {
    if mode == ResearchMode::Hybrid {
        session.enable_hybrid(
            model.ok_or("Для гибридного поиска нужна локальная модель.")?,
            embedding_budget(),
            now,
        )?;
    }
    Ok(())
}
pub fn admit(
    mut session: super::session::MemorySession,
    sessions: &MemorySessions,
    now: impl Fn() -> i64,
) -> Result<ResearchSession, String> {
    let mut descriptor = session.research_descriptor(&now)?;
    let id = sessions.insert(session, &now)?;
    if let Err(error) = sessions.with_session(&id, now, |_| Ok(())) {
        let _ = sessions.close(&id);
        return Err(error);
    }
    descriptor.id = id;
    Ok(descriptor)
}

pub fn with_local_model<R>(
    session: &mut MemorySession,
    models: &LocalModelService,
    now: impl Fn() -> i64,
    work: impl FnOnce(&mut MemorySession, &dyn EmbeddingModel) -> Result<R, String>,
) -> Result<R, String> {
    let cancellation = session.cancellation_handle();
    let deadline = session.research_deadline()?;
    let check = || {
        if cancellation.load(Ordering::SeqCst) {
            return Err("Сессия памяти отменена.".into());
        }
        if now() >= deadline {
            return Err("Время сессии памяти истекло.".into());
        }
        Ok(())
    };
    let id = format!(
        "memoryModel:{}",
        crate::connections::storage::random_id(24)?
    );
    models.with_runtime(&id, embedding_budget(), &check, |model, _| {
        work(session, model)
    })
}
