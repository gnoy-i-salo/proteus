use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

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
        let (id, kind, text, append) = if provider == "codex"
            && ["item.started", "item.updated", "item.completed"]
                .contains(&event["type"].as_str().unwrap_or(""))
            && event["item"]["type"] == "reasoning"
        {
            (
                event["item"]["id"].as_str()?,
                "summary",
                event["item"]["text"].as_str()?,
                false,
            )
        } else if provider == "kimi"
            && event["method"] == "session/update"
            && event["params"]["update"]["sessionUpdate"] == "agent_thought_chunk"
            && event["params"]["update"]["content"]["type"] == "text"
        {
            (
                "thinking",
                "thinking",
                event["params"]["update"]["content"]["text"].as_str()?,
                true,
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
        let available = (48 * 1024_usize).saturating_sub(if append { used } else { used - old });
        let mut end = text.len().min(available);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        self.truncated |= end < text.len();
        if end == 0 {
            return None;
        }
        let next = if append {
            format!(
                "{}{}",
                previous.map_or("", |index| self.blocks[index].text.as_str()),
                &text[..end]
            )
        } else {
            text[..end].to_owned()
        };
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

pub struct KimiSession {
    directory: String,
    model: String,
    prompt: String,
    session_id: Option<String>,
    phase: u64,
    pub answer: String,
    pub complete: bool,
}
impl KimiSession {
    pub fn new(directory: String, model: String, prompt: String) -> Self {
        Self {
            directory,
            model,
            prompt,
            session_id: None,
            phase: 1,
            answer: String::new(),
            complete: false,
        }
    }
    pub fn initialize() -> Value {
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{},"clientInfo":{"name":"proteus","version":"0.1.0"}}})
    }
    fn prompt_request(&self) -> Value {
        json!({"jsonrpc":"2.0","id":4,"method":"session/prompt","params":{"sessionId":self.session_id,"prompt":[{"type":"text","text":self.prompt}]}})
    }
    pub fn accept(&mut self, event: &Value) -> Result<Option<Value>, String> {
        if event.get("method").is_some() && event.get("id").is_some() {
            // This client grants no filesystem/terminal capabilities and declines unexpected approvals.
            return Ok(Some(if event["method"] == "session/request_permission" {
                json!({"jsonrpc":"2.0","id":event["id"],"result":{"outcome":{"outcome":"cancelled"}}})
            } else {
                json!({"jsonrpc":"2.0","id":event["id"],"error":{"code":-32601,"message":"Unsupported client capability"}})
            }));
        }
        if event["method"] == "session/update"
            && event["params"]["update"]["sessionUpdate"] == "tool_call"
        {
            self.answer.clear();
        }
        if event["method"] == "session/update"
            && event["params"]["update"]["sessionUpdate"] == "agent_message_chunk"
            && event["params"]["update"]["content"]["type"] == "text"
        {
            if let Some(text) = event["params"]["update"]["content"]["text"].as_str() {
                if self.answer.len() + text.len() > 1_000_000 {
                    return Err("Kimi answer exceeds the output limit".into());
                }
                self.answer.push_str(text);
            }
        }
        let Some(id) = event["id"].as_u64() else {
            return Ok(None);
        };
        if id != self.phase {
            return Ok(None);
        }
        if event.get("error").is_some() {
            return Err(format!(
                "Kimi ACP request failed (code {}). Check CLI login and the selected model.",
                event["error"]["code"]
            ));
        }
        match id {
            1 => {
                self.phase = 2;
                Ok(Some(
                    json!({"jsonrpc":"2.0","id":2,"method":"session/new","params":{"cwd":self.directory,"mcpServers":[]}}),
                ))
            }
            2 => {
                self.session_id = Some(
                    event["result"]["sessionId"]
                        .as_str()
                        .ok_or("Kimi did not return a session ID")?
                        .into(),
                );
                if self.model.is_empty() {
                    self.phase = 4;
                    Ok(Some(self.prompt_request()))
                } else {
                    self.phase = 3;
                    Ok(Some(
                        json!({"jsonrpc":"2.0","id":3,"method":"session/set_model","params":{"sessionId":self.session_id,"modelId":self.model}}),
                    ))
                }
            }
            3 => {
                self.phase = 4;
                Ok(Some(self.prompt_request()))
            }
            4 => {
                if event["result"]["stopReason"] != "end_turn" {
                    return Err("Kimi stopped before completing the answer".into());
                }
                if self.answer.trim().is_empty() {
                    return Err("Kimi returned an empty answer".into());
                }
                self.complete = true;
                self.phase = 5;
                Ok(Some(
                    json!({"jsonrpc":"2.0","id":5,"method":"session/close","params":{"sessionId":self.session_id}}),
                ))
            }
            _ => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn acp_negotiates_model_streams_answer_and_declines_unexpected_permissions() {
        let mut session = KimiSession::new(
            "/tmp/experiment".into(),
            "chosen-model".into(),
            "task".into(),
        );
        let next = session
            .accept(&json!({"id":1,"result":{}}))
            .unwrap()
            .unwrap();
        assert_eq!(next["method"], "session/new");
        let next = session
            .accept(&json!({"id":2,"result":{"sessionId":"session-1"}}))
            .unwrap()
            .unwrap();
        assert_eq!(next["method"], "session/set_model");
        assert_eq!(next["params"]["modelId"], "chosen-model");
        assert_eq!(
            session
                .accept(&json!({"id":3,"result":{}}))
                .unwrap()
                .unwrap()["method"],
            "session/prompt"
        );
        session.accept(&json!({"method":"session/update","params":{"update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Intermediate tool commentary"}}}})).unwrap();
        session.accept(&json!({"method":"session/update","params":{"update":{"sessionUpdate":"tool_call","toolCallId":"search-1"}}})).unwrap();
        for text in ["final ", "answer"] {
            session.accept(&json!({"method":"session/update","params":{"update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":text}}}})).unwrap();
        }
        let denied = session
            .accept(&json!({"id":99,"method":"session/request_permission"}))
            .unwrap()
            .unwrap();
        assert_eq!(denied["result"]["outcome"]["outcome"], "cancelled");
        assert_eq!(
            session
                .accept(&json!({"id":4,"result":{"stopReason":"end_turn"}}))
                .unwrap()
                .unwrap()["method"],
            "session/close"
        );
        assert!(session.complete);
        assert_eq!(session.answer, "final answer");
    }
    #[test]
    fn acp_fails_on_auth_errors_and_incomplete_output() {
        let mut session = KimiSession::new("/tmp/experiment".into(), String::new(), "task".into());
        assert!(session
            .accept(&json!({"id":1,"error":{"code":-32000,"message":"sensitive diagnostic"}}))
            .unwrap_err()
            .contains("-32000"));
        session.accept(&json!({"id":1,"result":{}})).unwrap();
        session
            .accept(&json!({"id":2,"result":{"sessionId":"session-1"}}))
            .unwrap();
        assert!(session
            .accept(&json!({"id":4,"result":{"stopReason":"cancelled"}}))
            .is_err());
        assert!(!session.complete);
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
    fn kimi_thought_chunks_are_separate_from_answer_and_diagnostics() {
        let mut reasoning = Reasoning::default();
        for text in ["Compare ", "the constraints."] {
            reasoning.accept("kimi", &json!({"method":"session/update","params":{"update":{"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":text}}}}));
        }
        reasoning.accept("kimi", &json!({"method":"session/update","params":{"update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"FINAL"}}}}));
        reasoning.accept("kimi", &json!({"error":{"message":"diagnostic"}}));
        assert_eq!(reasoning.blocks[0].text, "Compare the constraints.");
        assert_eq!(reasoning.blocks[0].kind, "thinking");
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
