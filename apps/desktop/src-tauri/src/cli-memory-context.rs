//! The CLI owns completion; callers can only supply a reference to a native lease.
use super::model::Request;
use crate::memory::session::{DispatchInput, InvocationBinding, MemorySession};
use std::sync::atomic::{AtomicBool, Ordering};
pub(super) struct Context<'a> {
    request: &'a Request,
    session: Option<&'a mut MemorySession>,
    now: &'a dyn Fn() -> i64,
    cancelled: &'a AtomicBool,
}
impl<'a> Context<'a> {
    pub fn new(
        request: &'a Request,
        session: Option<&'a mut MemorySession>,
        now: &'a dyn Fn() -> i64,
        cancelled: &'a AtomicBool,
    ) -> Result<Self, String> {
        if request.memory_context.is_some() != session.is_some() {
            return Err("Сессия контекста CLI недоступна.".into());
        }
        Ok(Self {
            request,
            session,
            now,
            cancelled,
        })
    }
    fn check_cancellation(&self) -> Result<(), String> {
        if self.cancelled.load(Ordering::SeqCst) {
            if let Some(session) = &self.session {
                session.cancel();
            }
            return Err("Отменено".into());
        }
        Ok(())
    }
    pub fn consume(&mut self) -> Result<(), String> {
        self.check_cancellation()?;
        if let (Some(session), Some(context)) = (&mut self.session, &self.request.memory_context) {
            let request = self.request;
            session.consume_model_context(
                &context.lease_id,
                DispatchInput {
                    invocation_id: &request.id,
                    binding: &InvocationBinding {
                        provider: request.provider.clone(),
                        model: request.model.clone(),
                        effort: request.effort.clone(),
                        operation: request.operation.clone(),
                    },
                    system_prompt: &request.system_prompt,
                    task: &request.task,
                },
                self.now,
            )?;
        }
        self.check_cancellation()
    }
    pub fn check(&mut self) -> Result<(), String> {
        self.check_cancellation()?;
        if let (Some(session), Some(context)) = (&mut self.session, &self.request.memory_context) {
            session.check_running_context(&context.lease_id, &self.request.id, self.now)?;
        }
        self.check_cancellation()
    }
    pub fn complete(&mut self) -> Result<(), String> {
        self.check_cancellation()?;
        if let (Some(session), Some(context)) = (&mut self.session, &self.request.memory_context) {
            session.complete_model_context(&context.lease_id, &self.request.id, self.now)?;
        }
        self.check_cancellation()
    }
}

impl Drop for Context<'_> {
    fn drop(&mut self) {
        // Cancellation may arrive during a capability probe or another early return.
        let _ = self.check_cancellation();
    }
}
