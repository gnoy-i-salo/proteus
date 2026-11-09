use super::MemorySession;
use crate::corpus_database::search::ResearchCatalog;
use crate::ingestion::grants::GrantUse;
use serde::Serialize;
use std::cell::RefCell;
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchCatalogResponse {
    #[serde(flatten)]
    pub catalog: ResearchCatalog,
    pub persona_id: String,
    pub policy_version: String,
}
impl MemorySession {
    pub fn research_catalog(
        &mut self,
        limit: usize,
        now: impl Fn() -> i64,
    ) -> Result<ResearchCatalogResponse, String> {
        if !(1..=10000).contains(&limit) || self.scope.persona.is_some() {
            return Err("Неверный размер каталога исследования.".into());
        }
        self.verify_current(&now)?;
        let scope = &self.scope;
        let ledger = RefCell::new(&mut self.ledger);
        let models = RefCell::new(self.models.as_mut());
        // This gate prevents grants/index changes during the metadata query.
        // Checkpoints verify cancellation and both clocks without reacquiring it.
        let catalog = scope
            .sources
            .with_ingestion_database(&scope.operation, |db| {
                let clock = || {
                    scope.operation.check()?;
                    let time = now();
                    scope.operation.check()?;
                    if time < 0 || time >= scope.deadline {
                        return Err("Разрешение источника истекло.".into());
                    }
                    Ok(time)
                };
                let check = || {
                    ledger.borrow_mut().verify(clock)?;
                    if let Some(models) = models.borrow_mut().as_mut() {
                        models.verify(&clock)?;
                    }
                    Ok(())
                };
                check()?;
                scope.authority.current(db, clock()?, GrantUse::Model)?;
                let pin = scope
                    .authority
                    .pin_index(db, &scope.source.snapshot_id, &check)?;
                if pin.receipt != scope.source {
                    return Err("Индекс сессии изменился.".into());
                }
                let allowed =
                    scope
                        .authority
                        .scope(&pin, &scope.persona_id, scope.period.clone())?;
                db.research_catalog(&allowed, &scope.source, limit, &check)
            })?;
        let result = ResearchCatalogResponse {
            catalog,
            persona_id: self.scope.persona_id.clone(),
            policy_version: self.scope.policy_version()?.into(),
        };
        self.verify_current(now)?;
        Ok(result)
    }
}
