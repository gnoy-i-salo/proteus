use super::job_budget::validate_selection_size;
use super::job_types::*;
use super::INVALID;
use std::collections::HashSet;
pub(super) fn validate_checkpoint(
    checkpoint: &JobCheckpoint,
    committed_batch: i64,
    staging_generation: i64,
) -> Result<(), String> {
    if checkpoint.adapter_version.is_empty()
        || checkpoint.adapter_version.len() > 256
        || checkpoint.committed_batch <= committed_batch
        || checkpoint.staging_generation < staging_generation
        || checkpoint.committed_batch > 9_007_199_254_740_991
        || checkpoint.staging_generation > 9_007_199_254_740_991
        || checkpoint
            .cursor
            .as_ref()
            .is_some_and(|cursor| cursor.len() > 8192)
    {
        return Err("Недопустимая контрольная точка задачи импорта.".into());
    }
    Ok(())
}
pub(super) fn validate_error(error: &JobError) -> Result<(), String> {
    if error.code.is_empty()
        || error.code.len() > 128
        || error.message.is_empty()
        || error.message.len() > 2048
    {
        return Err("Недопустимое описание ошибки задачи импорта.".into());
    }
    Ok(())
}
pub fn validate_budget(budget: &JobBudget) -> Result<(), String> {
    if [
        budget.max_input_bytes,
        budget.max_expanded_bytes,
        budget.max_documents,
        budget.max_duration_ms,
        budget.max_batch_documents,
        budget.max_batch_bytes,
    ]
    .iter()
    .any(|limit| *limit <= 0 || *limit > 9_007_199_254_740_991)
        || budget.max_batch_documents > budget.max_documents
        || budget.max_batch_bytes > budget.max_expanded_bytes
    {
        return Err("Недопустимый бюджет задачи импорта.".into());
    }
    Ok(())
}
pub(super) fn validate_input(input: &JobInput) -> Result<(), String> {
    if input.id.is_empty()
        || input.id.len() > 256
        || input.selection.preview_id.is_empty()
        || input.selection.preview_id.len() > 2048
    {
        return Err(INVALID.into());
    }
    validate_budget(&input.budget)?;
    if input.adapter_version.is_empty() || input.adapter_version.len() > 256 {
        return Err(INVALID.into());
    }
    if input
        .selection
        .resource_ids
        .iter()
        .collect::<HashSet<_>>()
        .len()
        != input.selection.resource_ids.len()
        || input
            .selection
            .bindings
            .iter()
            .map(|binding| &binding.id)
            .collect::<HashSet<_>>()
            .len()
            != input.selection.bindings.len()
        || input
            .selection
            .bindings
            .iter()
            .any(|binding| !input.selection.resource_ids.contains(&binding.resource_id))
        || input.selection.resource_ids.iter().any(|resource| {
            !input
                .selection
                .bindings
                .iter()
                .any(|binding| &binding.resource_id == resource)
        })
    {
        return Err("Привязки не соответствуют выбранным источникам импорта.".into());
    }
    if input.selection.resource_ids.is_empty() || input.selection.resource_ids.len() > 10000 {
        return Err("Недопустимый выбор задачи импорта.".into());
    }
    for resource in &input.selection.resource_ids {
        if resource.is_empty() || resource.len() > 2048 {
            return Err(INVALID.into());
        }
    }
    if input.selection.bindings.is_empty() || input.selection.bindings.len() > MAX_JOB_BINDINGS {
        return Err("Недопустимый выбор задачи импорта.".into());
    }
    validate_selection_size(&input.selection)?;
    for binding in &input.selection.bindings {
        validate_binding(binding)?;
    }
    Ok(())
}
pub fn validate_binding(binding: &JobBinding) -> Result<(), String> {
    if (binding.role == JobRole::Own && binding.target_author_id.is_none())
        || binding
            .allowed_context_author_ids
            .iter()
            .collect::<HashSet<_>>()
            .len()
            != binding.allowed_context_author_ids.len()
        || binding
            .target_author_id
            .as_ref()
            .is_some_and(|id| binding.allowed_context_author_ids.contains(id))
        || binding.schema_version != 1
        || !valid_binding_id(&binding.id)
        || !valid_binding_id(&binding.persona_id)
        || binding.resource_id.is_empty()
        || binding.resource_id.len() > 2048
        || !valid_binding_id(&binding.grant_id)
        || binding.policy_version.is_empty()
        || binding.policy_version.len() > 128
        || binding.allowed_context_author_ids.len() > 128
        || binding
            .allowed_context_author_ids
            .iter()
            .any(|author| !valid_binding_id(author))
    {
        return Err(INVALID.into());
    }
    if binding
        .target_author_id
        .as_ref()
        .is_some_and(|id| !valid_binding_id(id))
        || binding.account_id.as_ref().is_some_and(|id| id.len() > 256)
    {
        return Err(INVALID.into());
    }
    let parse = |value: &Option<String>| -> Result<Option<i64>, String> {
        match value {
            Some(raw) => {
                let millis = chrono::DateTime::parse_from_rfc3339(raw)
                    .map_err(|_| "Недопустимый период задачи импорта.".to_string())?
                    .timestamp_millis();
                Ok(Some(millis))
            }
            None => Ok(None),
        }
    };
    if let (Some(from), Some(to)) = (parse(&binding.period.from)?, parse(&binding.period.to)?) {
        if from > to {
            return Err("Недопустимый период задачи импорта.".into());
        }
    }
    Ok(())
}

fn valid_binding_id(value: &str) -> bool {
    !value
        .trim_matches(|character: char| character.is_whitespace() || character == '\u{feff}')
        .is_empty()
        && value.len() <= 128
        && !value.chars().any(char::is_control)
}
