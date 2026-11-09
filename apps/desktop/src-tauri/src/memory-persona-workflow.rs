//! Person admission is configured exclusively by the stored native manifest.
use super::{
    registry::MemorySessions,
    research_workflow::{self, ResearchMode, ResearchSession},
    session::{MemorySession, PersonaSessionInput},
    types::{self, PersonaManifest},
};
use crate::{
    ingestion::search_workflow::QueryPeriod, local_embedding::model::EmbeddingModel,
    sources::Sources,
};
use serde::{Deserialize, Serialize};
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenPersonaRequest {
    pub schema_version: u8,
    pub operation_id: String,
    pub job_id: String,
    pub persona_id: String,
    pub manifest_id: String,
    pub period: QueryPeriod,
}
impl OpenPersonaRequest {
    pub fn validate(&self) -> Result<(), String> {
        for value in [
            &self.operation_id,
            &self.job_id,
            &self.persona_id,
            &self.manifest_id,
        ] {
            types::validate_host_id(value, "область сессии Person")?;
        }
        if self.schema_version != 1
            || self.operation_id.len() > 128
            || self.job_id.len() > 128
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
            return Err("Неверные параметры сессии Person.".into());
        }
        Ok(())
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonaSession {
    #[serde(flatten)]
    pub session: ResearchSession,
    pub purpose: &'static str,
    pub manifest: PersonaManifest,
}
pub fn open(
    sources: &Sources,
    sessions: &MemorySessions,
    request: &OpenPersonaRequest,
    model: Option<&dyn EmbeddingModel>,
    now: impl Fn() -> i64,
) -> Result<PersonaSession, String> {
    let mut session = prepare(sources, request, &now)?;
    configure(&mut session, model, &now)?;
    admit(session, sessions, now)
}
pub fn prepare(
    sources: &Sources,
    request: &OpenPersonaRequest,
    now: impl Fn() -> i64,
) -> Result<MemorySession, String> {
    request.validate()?;
    let provider = crate::ingestion::job_workflow::job_provider(sources, &request.job_id)?;
    let operation = sources.begin_ingestion_operation(&request.operation_id, provider)?;
    MemorySession::bind_persona(
        sources.clone(),
        operation,
        PersonaSessionInput {
            job_id: &request.job_id,
            persona_id: &request.persona_id,
            manifest_id: &request.manifest_id,
            period: crate::corpus_database::search::SearchPeriod {
                from: request.period.from,
                to: request.period.to,
                include_undated: request.period.include_undated,
            },
        },
        now,
    )
}
pub fn configure(
    session: &mut MemorySession,
    model: Option<&dyn EmbeddingModel>,
    now: impl Fn() -> i64,
) -> Result<(), String> {
    if session
        .persona_manifest(&now)?
        .embedding_fingerprint
        .is_some()
    {
        research_workflow::configure(session, ResearchMode::Hybrid, model, &now)?;
    }
    session.verify_persona_runtime()?;
    session.verify_current(now)
}
pub fn admit(
    mut session: MemorySession,
    sessions: &MemorySessions,
    now: impl Fn() -> i64,
) -> Result<PersonaSession, String> {
    session.verify_persona_runtime()?;
    let manifest = session.persona_manifest(&now)?;
    Ok(PersonaSession {
        session: research_workflow::admit(session, sessions, now)?,
        purpose: "personaReply",
        manifest,
    })
}
