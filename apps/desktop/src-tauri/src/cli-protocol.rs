use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct ReasoningBlock {
    pub id: String,
    pub kind: String,
    pub text: String,
}
#[derive(Default)]
pub struct Reasoning {
    pub blocks: Vec<ReasoningBlock>,
    pub truncated: bool,
}
impl Reasoning {
    pub fn accept(&mut self, provider: &str, event: &Value) -> Option<ReasoningBlock> {
        let (id, kind, text) = if provider == "codex"
            && ["item.started", "item.updated", "item.completed"]
                .contains(&event["type"].as_str().unwrap_or(""))
            && event["item"]["type"] == "reasoning"
        {
            (
                event["item"]["id"].as_str()?,
                "summary",
                event["item"]["text"].as_str()?,
            )
        } else {
            return None;
        };
        if text.is_empty() {
            return None;
        }
        let previous = self.blocks.iter().position(|block| block.id == id);
        if previous.is_none() && self.blocks.len() >= 64 {
            self.truncated = true;
            return None;
        }
        let used: usize = self.blocks.iter().map(|block| block.text.len()).sum();
        let old = previous.map_or(0, |index| self.blocks[index].text.len());
        let available = (48 * 1024_usize).saturating_sub(used - old);
        let mut end = text.len().min(available);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        self.truncated |= end < text.len();
        if end == 0 {
            return None;
        }
        let next = text[..end].to_owned();
        if previous.is_some_and(|index| self.blocks[index].text == next) {
            return None;
        }
        let block = ReasoningBlock {
            id: id.into(),
            kind: kind.into(),
            text: next,
        };
        if let Some(index) = previous {
            self.blocks[index] = block.clone();
        } else {
            self.blocks.push(block.clone());
        }
        Some(block)
    }
}

#[derive(Default)]
pub struct KimiOutput {
    answer: String,
}
impl KimiOutput {
    pub fn accept(&mut self, event: &Value) -> Result<(), String> {
        if event["type"] == "error" {
            return Err(
                "Kimi CLI reported an error. Check CLI login, model and limits in the terminal."
                    .into(),
            );
        }
        if event["role"] == "tool"
            || (event["role"] == "meta" && event["type"] == "turn.step.retrying")
        {
            self.answer.clear();
        } else if event["role"] == "assistant" {
            if event
                .get("tool_calls")
                .is_some_and(|value| !value.as_array().is_some_and(Vec::is_empty))
            {
                self.answer.clear();
            } else {
                let text = event["content"].as_str().unwrap_or("");
                if text.len() > 1_000_000 {
                    return Err("Kimi answer exceeds the output limit".into());
                }
                self.answer = text.into();
            }
        }
        Ok(())
    }
    pub fn finish(self, successful_exit: bool) -> Result<String, String> {
        if !successful_exit || self.answer.trim().is_empty() {
            return Err("Kimi CLI did not complete a final answer".into());
        }
        Ok(self.answer)
    }
}

// Answers and CLI-exposed reasoning are parsed separately; diagnostics are never displayed.
pub(crate) fn answer_event(provider: &str, event: &Value) -> Option<String> {
    if provider == "codex"
        && event["type"] == "item.completed"
        && event["item"]["type"] == "agent_message"
    {
        return event["item"]["text"].as_str().map(str::to_owned);
    }
    None
}
pub(crate) fn parse_event(provider: &str, line: &str) -> Result<Option<Value>, String> {
    if line.trim().is_empty() {
        return Ok(None);
    }
    match serde_json::from_str::<Value>(line) {
        Ok(event) if provider != "kimi" || event.is_object() => Ok(Some(event)),
        _ if provider == "kimi" => Err("Kimi returned invalid JSONL output".into()),
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn kimi_print_discards_tool_commentary_and_retry_candidates() {
        for clearing in [
            json!({"role":"assistant","content":"Searching","tool_calls":[{"id":"search-1"}]}),
            json!({"role":"tool","content":"tool output"}),
            json!({"role":"meta","type":"turn.step.retrying","error_message":"diagnostic"}),
        ] {
            let mut output = KimiOutput::default();
            output
                .accept(&json!({"role":"assistant","content":"unfinished candidate"}))
                .unwrap();
            output.accept(&clearing).unwrap();
            assert!(output.finish(true).is_err());
        }
        let mut output = KimiOutput::default();
        output
            .accept(&json!({"role":"assistant","content":"Searching","tool_calls":[{}]}))
            .unwrap();
        output
            .accept(&json!({"role":"tool","content":"source text"}))
            .unwrap();
        output
            .accept(&json!({"role":"assistant","content":"final answer"}))
            .unwrap();
        output
            .accept(&json!({"role":"meta","type":"session.resume_hint","content":"metadata"}))
            .unwrap();
        assert_eq!(output.finish(true).unwrap(), "final answer");
    }
    #[test]
    fn kimi_print_requires_successful_exit_and_rejects_errors_and_oversized_answers() {
        let mut output = KimiOutput::default();
        output
            .accept(&json!({"role":"assistant","content":"partial answer"}))
            .unwrap();
        assert!(output.finish(false).is_err());
        let mut output = KimiOutput::default();
        assert!(output
            .accept(
                &json!({"type":"error","code":"session.occupied","message":"private diagnostic"})
            )
            .is_err());
        assert!(output
            .accept(&json!({"role":"assistant","content":"x".repeat(1_000_001)}))
            .is_err());
        assert!(KimiOutput::default().finish(true).is_err());
    }
    #[test]
    fn kimi_print_does_not_claim_unpublished_thinking_or_metadata_as_reasoning() {
        let mut reasoning = Reasoning::default();
        for event in [
            json!({"role":"assistant","content":"answer","reasoning":"not a public print event"}),
            json!({"role":"meta","type":"turn.step.retrying","error_message":"diagnostic"}),
            json!({"method":"session/update","params":{"update":{"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"obsolete ACP transport"}}}}),
        ] {
            assert!(reasoning.accept("kimi", &event).is_none());
        }
        assert!(reasoning.blocks.is_empty());
    }
    #[test]
    fn codex_snapshots_replace_instead_of_duplicate() {
        let mut reasoning = Reasoning::default();
        for (event_type, text) in [
            ("item.updated", "Compare"),
            ("item.completed", "Compare the options."),
        ] {
            reasoning.accept(
                "codex",
                &json!({"type":event_type,"item":{"id":"r1","type":"reasoning","text":text}}),
            );
        }
        assert_eq!(
            reasoning.blocks,
            vec![ReasoningBlock {
                id: "r1".into(),
                kind: "summary".into(),
                text: "Compare the options.".into()
            }]
        );
    }
    #[test]
    fn text_is_bounded_at_utf8_boundaries_and_raw_encrypted_data_is_ignored() {
        let mut reasoning = Reasoning::default();
        reasoning.accept("codex", &json!({"type":"item.completed","item":{"id":"raw","type":"reasoning","encrypted_content":"secret"}}));
        assert!(reasoning.blocks.is_empty());
        reasoning.accept("codex", &json!({"type":"item.completed","item":{"id":"long","type":"reasoning","text":"я".repeat(50000)}}));
        assert!(reasoning.truncated);
        assert!(reasoning.blocks[0].text.len() <= 48 * 1024);
        assert!(reasoning.blocks[0]
            .text
            .is_char_boundary(reasoning.blocks[0].text.len()));
    }
}
