//! Ordered token text projection and structural closure validation.
use super::*;

impl<'a> Sink<'a> {
    fn on_end_tag(&self, name: &str) {
        let mut stack = self.stack.borrow_mut();
        let Some(position) = stack.iter().rposition(|frame| frame.name == name) else {
            if stack.iter().any(|frame| frame.kind.is_structure()) {
                self.fail(format!(
                    "{ERR_STRUCTURE} Непарный закрывающий тег </{name}> внутри сообщения."
                ));
            }
            return;
        };
        if stack[position + 1..]
            .iter()
            .any(|frame| frame.kind.is_structure())
        {
            self.fail(format!(
                "{ERR_STRUCTURE} Некорректная вложенность: закрывающий тег </{name}> пересекает структуру сообщения."
            ));
            return;
        }
        let popped = stack.split_off(position);
        drop(stack);
        for frame in popped.iter().rev() {
            self.after_pop(frame);
        }
    }

    fn after_pop(&self, frame: &Frame) {
        match frame.kind {
            FrameKind::Message => self.close_message(),
            FrameKind::Forward => self.close_forward(),
            FrameKind::AuthorLink => {
                let mut messages = self.messages.borrow_mut();
                if let Some(message) = messages.last_mut() {
                    message.after_author = true;
                }
            }
            FrameKind::Edited => {
                let mut messages = self.messages.borrow_mut();
                if let Some(message) = messages.last_mut() {
                    let trimmed = message.edit_text.trim();
                    if !trimmed.is_empty() && trimmed.len() <= MAX_EDIT_LABEL_BYTES {
                        message.edit_text = trimmed.to_string();
                    }
                }
            }
            FrameKind::Kludge => {
                let mut messages = self.messages.borrow_mut();
                if let Some(note) = messages
                    .last_mut()
                    .and_then(|message| message.kludges.last_mut())
                {
                    let trimmed = note.trim();
                    *note = trimmed.to_string();
                }
            }
            FrameKind::AttachmentDescription => {
                let mut messages = self.messages.borrow_mut();
                if let Some(attachment) = messages
                    .last_mut()
                    .and_then(|message| message.attachments.last_mut())
                {
                    if let Some(description) = attachment.description.as_mut() {
                        let trimmed = description.trim();
                        if trimmed.len() > MAX_ATTACHMENT_TEXT_BYTES {
                            self.fail(ERR_BODY);
                        } else {
                            *description = trimmed.to_string();
                        }
                    }
                }
            }
            FrameKind::ForwardAuthorLink => {
                let mut messages = self.messages.borrow_mut();
                if let Some(context) = messages
                    .last_mut()
                    .and_then(|message| message.forwards.last_mut())
                {
                    let trimmed = context.author_label.trim();
                    context.author_label = trimmed.to_string();
                }
            }
            _ => {}
        }
    }

    fn close_message(&self) {
        let mut messages = self.messages.borrow_mut();
        let Some(builder) = messages.last_mut() else {
            return;
        };
        if !builder.header_seen {
            self.fail(format!("{ERR_STRUCTURE} Неполное сообщение без заголовка."));
            return;
        }
        if !builder.author_seen {
            self.fail(format!(
                "{ERR_STRUCTURE} Неполный заголовок сообщения: нет ссылки автора."
            ));
            return;
        }
        if !builder.body_seen {
            self.fail(format!("{ERR_STRUCTURE} Неполное сообщение без тела."));
            return;
        }
        if trim_date(&builder.date_raw).is_empty() {
            self.fail(format!(
                "{ERR_STRUCTURE} В заголовке сообщения нет даты после ссылки автора."
            ));
        }
    }

    fn close_forward(&self) {
        let mut messages = self.messages.borrow_mut();
        let Some(context) = messages
            .last_mut()
            .and_then(|message| message.forwards.last_mut())
        else {
            return;
        };
        if !context.header_seen {
            self.fail(format!(
                "{ERR_STRUCTURE} Пересланное сообщение без заголовка не поддерживается."
            ));
            return;
        }
        if !context.body_seen {
            self.fail(format!(
                "{ERR_STRUCTURE} Пересланное сообщение без тела не поддерживается."
            ));
        }
    }

    // ---- character tokens --------------------------------------------------

    fn push_text(&self, text: &str) {
        if text.is_empty() || self.failed() {
            return;
        }
        let (top, enclosing_forward, after_author) = {
            let stack = self.stack.borrow();
            let top = stack.last().map(|frame| frame.kind);
            let enclosing = stack
                .iter()
                .rev()
                .find(|frame| matches!(frame.kind, FrameKind::Message | FrameKind::Forward))
                .is_some_and(|frame| frame.kind == FrameKind::Forward);
            let after_author = self
                .messages
                .borrow()
                .last()
                .is_some_and(|message| message.after_author);
            (top, enclosing, after_author)
        };
        let target = match top {
            None
            | Some(FrameKind::Generic)
            | Some(FrameKind::Skip)
            | Some(FrameKind::AttachmentLink)
            | Some(FrameKind::ForwardHeader) => TextTarget::Discard,
            Some(FrameKind::Message) | Some(FrameKind::Forward) | Some(FrameKind::Attachment) => {
                TextTarget::BlockLevel
            }
            Some(FrameKind::Header) => TextTarget::HeaderText,
            Some(FrameKind::AuthorLink) => TextTarget::Author {
                forward: enclosing_forward,
            },
            Some(FrameKind::Edited) => TextTarget::Edited,
            Some(FrameKind::Body) => TextTarget::Body {
                forward: enclosing_forward,
            },
            Some(FrameKind::ForwardBody) => TextTarget::Body { forward: true },
            Some(FrameKind::ForwardAuthorLink) => TextTarget::Author { forward: true },
            Some(FrameKind::AttachmentDescription) => TextTarget::AttachmentDescription,
            Some(FrameKind::Kludge) => TextTarget::Kludge,
        };
        if matches!(
            target,
            TextTarget::HeaderText | TextTarget::Author { forward: false } | TextTarget::Edited
        ) {
            self.current_message(|message| {
                self.append(
                    &mut message.header_text,
                    text,
                    MAX_HEADER_TEXT_BYTES,
                    ERR_BODY,
                )
            });
        }
        match target {
            TextTarget::Discard => {}
            TextTarget::BlockLevel => {
                if !text.trim().is_empty() {
                    self.fail(format!(
                        "{ERR_STRUCTURE} Текст вне заголовка и тела сообщения."
                    ));
                }
            }
            TextTarget::HeaderText => {
                let mut messages = self.messages.borrow_mut();
                let Some(message) = messages.last_mut() else {
                    return;
                };
                self.append(&mut message.date_raw, text, MAX_HEADER_TEXT_BYTES, ERR_BODY);
                if !after_author && !text.trim().is_empty() {
                    self.fail(format!(
                        "{ERR_STRUCTURE} Текст в заголовке перед ссылкой автора."
                    ));
                }
            }
            TextTarget::Author { forward } => {
                let mut messages = self.messages.borrow_mut();
                let Some(message) = messages.last_mut() else {
                    return;
                };
                if forward {
                    if let Some(context) = message.forwards.last_mut() {
                        self.append(
                            &mut context.author_label,
                            text,
                            MAX_AUTHOR_LABEL_BYTES,
                            ERR_BODY,
                        );
                    }
                } else {
                    self.append(
                        &mut message.author_label,
                        text,
                        MAX_AUTHOR_LABEL_BYTES,
                        ERR_BODY,
                    );
                }
            }
            TextTarget::Edited => {
                let mut messages = self.messages.borrow_mut();
                let Some(message) = messages.last_mut() else {
                    return;
                };
                self.append(&mut message.edit_text, text, MAX_EDIT_LABEL_BYTES, ERR_BODY);
            }
            TextTarget::Body { forward } => {
                let mut messages = self.messages.borrow_mut();
                let Some(message) = messages.last_mut() else {
                    return;
                };
                if forward {
                    if let Some(context) = message.forwards.last_mut() {
                        self.append(
                            &mut context.body_text,
                            text,
                            MAX_FORWARDED_BODY_BYTES,
                            ERR_BODY,
                        );
                    }
                } else {
                    self.append(&mut message.body_text, text, MAX_BODY_BYTES, ERR_BODY);
                }
            }
            TextTarget::AttachmentDescription => {
                let mut messages = self.messages.borrow_mut();
                let Some(attachment) = messages
                    .last_mut()
                    .and_then(|message| message.attachments.last_mut())
                else {
                    return;
                };
                if let Some(description) = attachment.description.as_mut() {
                    self.append(description, text, MAX_ATTACHMENT_TEXT_BYTES, ERR_BODY);
                }
            }
            TextTarget::Kludge => {
                let mut messages = self.messages.borrow_mut();
                let Some(note) = messages
                    .last_mut()
                    .and_then(|message| message.kludges.last_mut())
                else {
                    return;
                };
                self.append(note, text, MAX_KLUDGE_TEXT_BYTES, ERR_BODY);
            }
        }
    }
}

impl<'a> TokenSink for Sink<'a> {
    type Handle = ();

    fn process_token(&self, token: Token, _line_number: u64) -> TokenSinkResult<()> {
        if self.failed() {
            return TokenSinkResult::Continue;
        }
        let action = match token {
            TagToken(ref tag) => match tag.kind {
                TagKind::StartTag => self.on_start_tag(tag),
                TagKind::EndTag => {
                    self.on_end_tag(tag_name(tag));
                    Action::Continue
                }
            },
            Token::CharacterTokens(text) => {
                self.push_text(&text);
                Action::Continue
            }
            Token::NullCharacterToken | ParseError(_) => {
                self.fail(format!("{ERR_STRUCTURE} Некорректная HTML-разметка."));
                Action::Continue
            }
            // End of input is a stream boundary, not parse work; the caps
            // above bound the events that actually drive the grammar.
            Token::EOFToken => return TokenSinkResult::Continue,
            _ => Action::Continue,
        };
        if !self.failed() {
            let events = self.events.get() + 1;
            self.events.set(events);
            if events > MAX_TOKENIZER_EVENTS {
                self.fail(ERR_EVENTS);
            } else if events.is_multiple_of(CHECKPOINT_EVENT_INTERVAL) {
                if let Err(error) = (self.checkpoint)() {
                    self.fail(error);
                }
            }
        }
        match action {
            Action::Continue => TokenSinkResult::Continue,
            Action::Raw(raw) => TokenSinkResult::RawData(raw),
        }
    }
}
