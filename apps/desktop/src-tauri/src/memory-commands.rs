//! Thin IPC adapters. Only the host opens sessions; native scopes and budgets
//! remain authoritative for every command and CLI dispatch.
use super::{
    context_wire::{ModelContextLease, ModelContextRequest},
    publication::ResearchPublicationReceipt,
    registry::MemorySessions,
    research_draft::ResearchPublicationDraft,
    research_workflow::{self, OpenResearchRequest, ResearchMode, ResearchSession},
    session::{MemorySession, RecordedRetrieval, ResearchCatalogResponse},
};
use crate::{local_embedding::service::LocalModelService, sources::Sources};
use serde::Serialize;
use std::sync::Arc;

fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
pub struct RetrievalResponse(Arc<RecordedRetrieval>);
impl Serialize for RetrievalResponse {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.as_ref().serialize(serializer)
    }
}
async fn in_session<R: Send + 'static>(
    sessions: Arc<MemorySessions>,
    id: String,
    work: impl FnOnce(&mut MemorySession) -> Result<R, String> + Send + 'static,
) -> Result<R, String> {
    tauri::async_runtime::spawn_blocking(move || sessions.with_session(&id, now, work))
        .await
        .map_err(|_| "Не удалось выполнить операцию памяти.".to_string())?
}
#[tauri::command]
pub async fn open_research_session(
    request: OpenResearchRequest,
    sources: tauri::State<'_, Sources>,
    sessions: tauri::State<'_, Arc<MemorySessions>>,
    models: tauri::State<'_, Arc<LocalModelService>>,
) -> Result<ResearchSession, String> {
    request.validate()?;
    let sources = sources.inner().clone();
    let sessions = sessions.inner().clone();
    let models = models.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Register the cancellable source operation before any cold model load.
        let mut session = research_workflow::prepare(&sources, &request, now)?;
        if request.mode == ResearchMode::Hybrid {
            research_workflow::with_local_model(&mut session, &models, now, |session, model| {
                research_workflow::configure(session, request.mode, Some(model), now)
            })?;
        }
        // Admission follows the runtime's final check, so a late loading failure
        // cannot leave an undisclosed session in the registry.
        research_workflow::admit(session, &sessions, now)
    })
    .await
    .map_err(|_| "Не удалось открыть исследовательскую сессию.".to_string())?
}
#[tauri::command]
pub async fn open_fabric_session(
    request: OpenResearchRequest,
    sources: tauri::State<'_, Sources>,
    sessions: tauri::State<'_, Arc<MemorySessions>>,
) -> Result<research_workflow::FabricSession, String> {
    request.validate()?;
    let sources = sources.inner().clone();
    let sessions = sessions.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        research_workflow::open_fabric(&sources, &sessions, &request, now)
    })
    .await
    .map_err(|_| "Не удалось открыть сессию Person Fabric.".to_string())?
}
#[tauri::command]
pub async fn reserve_fabric_memory_context(
    session_id: String,
    request: super::session::FabricContextRequest,
    sessions: tauri::State<'_, Arc<MemorySessions>>,
) -> Result<ModelContextLease, String> {
    request.validate()?;
    in_session(sessions.inner().clone(), session_id, move |session| {
        session.reserve_fabric_context(&request, now)
    })
    .await
}
#[tauri::command]
pub fn close_memory_session(
    session_id: String,
    sessions: tauri::State<'_, Arc<MemorySessions>>,
) -> Result<bool, String> {
    sessions.close(&session_id)
}
#[tauri::command]
pub async fn recall_memory(
    session_id: String,
    query_text: String,
    sessions: tauri::State<'_, Arc<MemorySessions>>,
    models: tauri::State<'_, Arc<LocalModelService>>,
) -> Result<RetrievalResponse, String> {
    let models = models.inner().clone();
    in_session(sessions.inner().clone(), session_id, move |session| {
        let result = if session.uses_hybrid() {
            research_workflow::with_local_model(session, &models, now, |session, model| {
                session.recall_recorded(&query_text, Some(model), now)
            })?
        } else {
            session.recall_recorded(&query_text, None, now)?
        };
        Ok(RetrievalResponse(result))
    })
    .await
}
#[tauri::command]
pub async fn research_memory_catalog(
    session_id: String,
    limit: usize,
    sessions: tauri::State<'_, Arc<MemorySessions>>,
) -> Result<ResearchCatalogResponse, String> {
    in_session(sessions.inner().clone(), session_id, move |session| {
        session.research_catalog(limit, now)
    })
    .await
}
#[tauri::command]
pub async fn read_memory_episode(
    session_id: String,
    episode_id: String,
    sessions: tauri::State<'_, Arc<MemorySessions>>,
) -> Result<RetrievalResponse, String> {
    in_session(sessions.inner().clone(), session_id, move |session| {
        session
            .read_episode_recorded(&episode_id, now)
            .map(RetrievalResponse)
    })
    .await
}
#[tauri::command]
pub async fn reserve_memory_context(
    session_id: String,
    request: ModelContextRequest,
    sessions: tauri::State<'_, Arc<MemorySessions>>,
) -> Result<ModelContextLease, String> {
    request.validate()?;
    in_session(sessions.inner().clone(), session_id, move |session| {
        session.reserve_context(&request, now)
    })
    .await
}
#[tauri::command]
pub async fn publish_memory_research(
    session_id: String,
    draft: ResearchPublicationDraft,
    sessions: tauri::State<'_, Arc<MemorySessions>>,
) -> Result<ResearchPublicationReceipt, String> {
    draft.validate()?;
    in_session(sessions.inner().clone(), session_id, move |session| {
        session.publish_research(&draft, now)
    })
    .await
}
