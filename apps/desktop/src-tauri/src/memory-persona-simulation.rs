//! Persist generated dialogue only after exact native CLI completion proof.
use super::MemorySession;
use crate::{
    corpus_database::simulation::{SimulationMemoryDraft, SimulationMemoryEntry, SimulationRole},
    ingestion::grants::GrantUse,
    memory::simulation::AppendSimulationRequest,
};
use sha2::{Digest, Sha256};
use std::cell::RefCell;
const INVALID: &str = "Запись симуляции не соответствует сессии Person.";
#[derive(Clone)]
pub(super) struct SimulationMemo {
    hash: [u8; 32],
    id: String,
    created_at: String,
}
impl MemorySession {
    pub fn append_simulation(
        &mut self,
        request: &AppendSimulationRequest,
        now: impl Fn() -> i64,
    ) -> Result<SimulationMemoryEntry, String> {
        request.validate()?;
        self.verify_persona_runtime()?;
        self.verify_current(&now)?;
        let persona = self.scope.persona.as_ref().ok_or(INVALID)?;
        let manifest_id = persona.pin.bundle().manifest.id.clone();
        let previous = persona.simulation.clone();
        self.check_completed_model_output(
            &request.context_lease_id,
            &request.invocation_id,
            &request.answer,
            &now,
        )?;
        let text = request.reply_text()?;
        let hash: [u8; 32] =
            Sha256::digest(serde_json::to_vec(request).map_err(|_| INVALID)?).into();
        let memo = match previous {
            Some(memo) if memo.hash == hash => memo,
            Some(_) => return Err("Сессия уже сохранила другую реплику симуляции.".into()),
            None => SimulationMemo {
                hash,
                id: format!("simulation:{}", crate::connections::storage::random_id(24)?),
                created_at: chrono::DateTime::<chrono::Utc>::from_timestamp_millis(
                    self.ledger.verify(|| self.scope.checked_time(&now))?,
                )
                .ok_or(INVALID)?
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            },
        };
        let draft = SimulationMemoryDraft {
            schema_version: 1,
            experiment_id: request.experiment_id.clone(),
            manifest_id,
            trial_id: request.trial_id.clone(),
            role: SimulationRole::Simulation,
            text,
        };
        self.verify_current(&now)?;
        let scope = &self.scope;
        let persona = scope.persona.as_ref().ok_or(INVALID)?;
        let ledger = RefCell::new(&mut self.ledger);
        let models = RefCell::new(self.models.as_mut().ok_or(INVALID)?);
        let entry = scope
            .sources
            .with_ingestion_database(&scope.operation, |db| {
                // The source gate stays held through the transaction. Check time and
                // cancellation inside it without recursively acquiring that gate.
                let clock = || scope.checked_time(&now);
                let check = || {
                    ledger.borrow_mut().verify(clock)?;
                    models.borrow_mut().verify(&clock)?;
                    Ok(())
                };
                check()?;
                scope.authority.current(db, clock()?, GrantUse::Model)?;
                if db
                    .active_search_receipt(&scope.source.snapshot_id)?
                    .as_ref()
                    != Some(&scope.source)
                {
                    return Err(INVALID.into());
                }
                db.verify_manifest_pin(&persona.pin, &check)?;
                if let Some(vectors) = &persona.vectors {
                    if db
                        .active_vector_receipt(&scope.source.snapshot_id, &check)?
                        .as_ref()
                        != Some(vectors)
                    {
                        return Err(INVALID.into());
                    }
                }
                db.append_simulation_checked(&draft, &memo.id, &memo.created_at, &check)
            })?;
        // A commit stays committed even if a late revocation prevents disclosure.
        self.scope.persona.as_mut().ok_or(INVALID)?.simulation = Some(memo);
        self.verify_current(now)?;
        Ok(entry)
    }
}
