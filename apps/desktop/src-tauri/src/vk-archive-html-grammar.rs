//! Supported VK message element grammar; no acquisition or I/O.
use super::*;

impl<'a> Sink<'a> {
    // ---- start tag grammar -------------------------------------------------

    pub(super) fn on_start_tag(&self, tag: &Tag) -> Action {
        let name = tag_name(tag);
        let top = self.stack.borrow().last().map(|frame| frame.kind);
        match top {
            None | Some(FrameKind::Generic) => self.start_outside(tag),
            Some(FrameKind::Skip) => Action::Continue,
            Some(FrameKind::Message) => self.start_in_message(tag),
            Some(FrameKind::Header) => self.start_in_header(tag),
            Some(FrameKind::AuthorLink)
            | Some(FrameKind::Edited)
            | Some(FrameKind::AttachmentLink)
            | Some(FrameKind::AttachmentDescription)
            | Some(FrameKind::Kludge)
            | Some(FrameKind::ForwardAuthorLink) => {
                self.fail(format!(
                    "{ERR_STRUCTURE} Вложенный элемент <{name}> внутри текстового блока сообщения."
                ));
                Action::Continue
            }
            Some(FrameKind::Body) => self.start_in_body(tag, false),
            Some(FrameKind::Attachment) => self.start_in_attachment(tag),
            Some(FrameKind::Forward) => self.start_in_forward(tag),
            Some(FrameKind::ForwardHeader) => self.start_in_forward_header(tag),
            Some(FrameKind::ForwardBody) => self.start_in_body(tag, true),
        }
    }

    fn start_outside(&self, tag: &Tag) -> Action {
        let name = tag_name(tag);
        if is_void(name) {
            return Action::Continue;
        }
        if name == "div" && has_class(tag, "message") {
            return self.start_message(tag);
        }
        if let Some(raw) = raw_kind(name) {
            if self.push_frame(name, FrameKind::Skip) {
                return Action::Raw(raw);
            }
            return Action::Continue;
        }
        let _ = self.push_frame(name, FrameKind::Generic);
        Action::Continue
    }

    fn start_message(&self, tag: &Tag) -> Action {
        if self.messages.borrow().len() >= MAX_MESSAGES {
            self.fail(ERR_MESSAGES);
            return Action::Continue;
        }
        let external_id = match attr(tag, "data-id") {
            None => None,
            Some(value) => {
                let id = value.to_string();
                if id.trim().is_empty()
                    || id.len() > MAX_EXTERNAL_ID_BYTES
                    || id.chars().any(char::is_control)
                {
                    self.fail(format!("{ERR_STRUCTURE} Некорректный атрибут data-id."));
                    return Action::Continue;
                }
                if !self.seen_ids.borrow_mut().insert(id.clone()) {
                    self.fail(format!(
                        "{ERR_STRUCTURE} Повторяющийся data-id \"{id}\" на странице."
                    ));
                    return Action::Continue;
                }
                Some(id)
            }
        };
        if !self.push_frame("div", FrameKind::Message) {
            return Action::Continue;
        }
        let ordinal = self.messages.borrow().len() as u32 + 1;
        self.messages
            .borrow_mut()
            .push(MessageBuilder::new(ordinal, external_id));
        Action::Continue
    }

    fn start_in_message(&self, tag: &Tag) -> Action {
        let name = tag_name(tag);
        if name == "div" {
            if has_class(tag, "message") {
                self.fail(format!(
                    "{ERR_STRUCTURE} Вложенное сообщение вне тела сообщения."
                ));
                return Action::Continue;
            }
            let (header_seen, body_seen) = {
                let messages = self.messages.borrow();
                let Some(message) = messages.last() else {
                    return Action::Continue;
                };
                (message.header_seen, message.body_seen)
            };
            if has_class(tag, "message__header") {
                if header_seen {
                    self.fail(format!("{ERR_STRUCTURE} Повторный заголовок сообщения."));
                    return Action::Continue;
                }
                if self.push_frame("div", FrameKind::Header) {
                    if let Some(message) = self.messages.borrow_mut().last_mut() {
                        message.header_seen = true;
                    }
                }
                return Action::Continue;
            }
            if !header_seen {
                self.fail(format!(
                    "{ERR_STRUCTURE} Ожидался заголовок сообщения (message__header)."
                ));
                return Action::Continue;
            }
            if body_seen {
                self.fail(format!(
                    "{ERR_STRUCTURE} Неподдерживаемый элемент в сообщении после тела."
                ));
                return Action::Continue;
            }
            if self.push_frame("div", FrameKind::Body) {
                if let Some(message) = self.messages.borrow_mut().last_mut() {
                    message.body_seen = true;
                }
            }
            return Action::Continue;
        }
        if let Some(raw) = raw_kind(name) {
            if self.push_frame(name, FrameKind::Skip) {
                return Action::Raw(raw);
            }
            return Action::Continue;
        }
        self.fail(format!(
            "{ERR_STRUCTURE} Неподдерживаемый элемент <{name}> в сообщении."
        ));
        Action::Continue
    }

    fn start_in_header(&self, tag: &Tag) -> Action {
        let name = tag_name(tag);
        if name == "a" {
            let href = match attr(tag, "href") {
                Some(value)
                    if !value.is_empty()
                        && value.len() <= MAX_AUTHOR_URL_BYTES
                        && !value.chars().any(char::is_control) =>
                {
                    value.to_string()
                }
                _ => {
                    self.fail(format!(
                        "{ERR_STRUCTURE} У автора сообщения некорректная или отсутствующая ссылка."
                    ));
                    return Action::Continue;
                }
            };
            let author_seen = self
                .messages
                .borrow()
                .last()
                .is_some_and(|message| message.author_seen);
            if author_seen {
                self.fail(format!(
                    "{ERR_STRUCTURE} Повторная ссылка автора в заголовке сообщения."
                ));
                return Action::Continue;
            }
            if self.push_frame("a", FrameKind::AuthorLink) {
                if let Some(message) = self.messages.borrow_mut().last_mut() {
                    message.author_seen = true;
                    message.author_url = Some(href);
                }
            }
            return Action::Continue;
        }
        if has_class(tag, "message-edited") {
            let edit_seen = self
                .messages
                .borrow()
                .last()
                .is_some_and(|message| message.edit_seen);
            if edit_seen {
                self.fail(format!(
                    "{ERR_STRUCTURE} Повторная метка редактирования в заголовке сообщения."
                ));
                return Action::Continue;
            }
            if self.push_frame(name, FrameKind::Edited) {
                if let Some(message) = self.messages.borrow_mut().last_mut() {
                    message.edit_seen = true;
                    message.edited = true;
                }
            }
            return Action::Continue;
        }
        if let Some(raw) = raw_kind(name) {
            if self.push_frame(name, FrameKind::Skip) {
                return Action::Raw(raw);
            }
            return Action::Continue;
        }
        self.fail(format!(
            "{ERR_STRUCTURE} Неподдерживаемый элемент <{name}> в заголовке сообщения."
        ));
        Action::Continue
    }

    fn start_in_body(&self, tag: &Tag, forward: bool) -> Action {
        let name = tag_name(tag);
        if name == "br" {
            if forward {
                self.current_forward(|context| {
                    self.append(
                        &mut context.body_text,
                        "\n",
                        MAX_FORWARDED_BODY_BYTES,
                        ERR_BODY,
                    )
                });
            } else {
                self.current_message(|message| {
                    self.append(&mut message.body_text, "\n", MAX_BODY_BYTES, ERR_BODY)
                });
            }
            return Action::Continue;
        }
        if name == "div" {
            self.separate_body_block(forward);
            if self.failed() {
                return Action::Continue;
            }
            if has_class(tag, "attachment") {
                if forward {
                    self.fail(format!(
                        "{ERR_STRUCTURE} Вложения внутри пересланного контекста не поддерживаются."
                    ));
                    return Action::Continue;
                }
                let full = self.messages.borrow().last().is_some_and(|message| {
                    message.attachments.len() >= MAX_ATTACHMENTS_PER_MESSAGE
                });
                if full {
                    self.fail(format!(
                        "{ERR_STRUCTURE} Превышен лимит вложений сообщения."
                    ));
                    return Action::Continue;
                }
                if self.push_frame("div", FrameKind::Attachment) {
                    if let Some(message) = self.messages.borrow_mut().last_mut() {
                        message.attachments.push(AttachmentBuilder {
                            link: None,
                            description: None,
                        });
                    }
                }
                return Action::Continue;
            }
            if has_class(tag, "kludges") || has_class(tag, "kludge") {
                if forward {
                    self.fail(format!(
                        "{ERR_STRUCTURE} Служебные пометки внутри пересланного контекста не поддерживаются."
                    ));
                    return Action::Continue;
                }
                let full = self
                    .messages
                    .borrow()
                    .last()
                    .is_some_and(|message| message.kludges.len() >= MAX_KLUDGES_PER_MESSAGE);
                if full {
                    self.fail(format!(
                        "{ERR_STRUCTURE} Превышен лимит служебных пометок сообщения."
                    ));
                    return Action::Continue;
                }
                if self.push_frame("div", FrameKind::Kludge) {
                    if let Some(message) = self.messages.borrow_mut().last_mut() {
                        message.kludges.push(String::new());
                    }
                }
                return Action::Continue;
            }
            if has_class(tag, "message") {
                if forward {
                    self.fail(format!(
                        "{ERR_STRUCTURE} Вложенная пересылка внутри пересланного контекста не поддерживается."
                    ));
                    return Action::Continue;
                }
                let full = self
                    .messages
                    .borrow()
                    .last()
                    .is_some_and(|message| message.forwards.len() >= MAX_FORWARDED_CONTEXTS);
                if full {
                    self.fail(format!(
                        "{ERR_STRUCTURE} Превышен лимит пересланных контекстов сообщения."
                    ));
                    return Action::Continue;
                }
                if self.push_frame("div", FrameKind::Forward) {
                    if let Some(message) = self.messages.borrow_mut().last_mut() {
                        message.has_forward = true;
                        message.forwards.push(ForwardBuilder::new());
                    }
                }
                return Action::Continue;
            }
            self.fail(format!(
                "{ERR_STRUCTURE} Неподдерживаемый вложенный блок в теле сообщения."
            ));
            return Action::Continue;
        }
        if let Some(raw) = raw_kind(name) {
            if self.push_frame(name, FrameKind::Skip) {
                return Action::Raw(raw);
            }
            return Action::Continue;
        }
        self.fail(format!(
            "{ERR_STRUCTURE} Неподдерживаемый элемент <{name}> в теле сообщения."
        ));
        Action::Continue
    }

    fn separate_body_block(&self, forward: bool) {
        let mut messages = self.messages.borrow_mut();
        let Some(message) = messages.last_mut() else {
            return;
        };
        let body = if forward {
            let Some(context) = message.forwards.last_mut() else {
                return;
            };
            &mut context.body_text
        } else {
            &mut message.body_text
        };
        if !body.is_empty() && !body.ends_with('\n') {
            self.append(
                body,
                "\n",
                if forward {
                    MAX_FORWARDED_BODY_BYTES
                } else {
                    MAX_BODY_BYTES
                },
                ERR_BODY,
            );
        }
    }

    fn start_in_attachment(&self, tag: &Tag) -> Action {
        let name = tag_name(tag);
        if name == "a" && has_class(tag, "attachment__link") {
            let href = match attr(tag, "href") {
                Some(value)
                    if !value.is_empty()
                        && value.len() <= MAX_ATTACHMENT_LINK_BYTES
                        && !value.chars().any(char::is_control) =>
                {
                    value.to_string()
                }
                _ => {
                    self.fail(format!(
                        "{ERR_STRUCTURE} У ссылки вложения некорректный или отсутствующий адрес."
                    ));
                    return Action::Continue;
                }
            };
            let mut messages = self.messages.borrow_mut();
            let Some(attachment) = messages
                .last_mut()
                .and_then(|message| message.attachments.last_mut())
            else {
                return Action::Continue;
            };
            if attachment.link.is_some() {
                self.fail(format!("{ERR_STRUCTURE} Повторная ссылка вложения."));
                return Action::Continue;
            }
            attachment.link = Some(href);
            drop(messages);
            let _ = self.push_frame("a", FrameKind::AttachmentLink);
            return Action::Continue;
        }
        if name == "div" && has_class(tag, "attachment__description") {
            let mut messages = self.messages.borrow_mut();
            let Some(attachment) = messages
                .last_mut()
                .and_then(|message| message.attachments.last_mut())
            else {
                return Action::Continue;
            };
            if attachment.description.is_some() {
                self.fail(format!("{ERR_STRUCTURE} Повторное описание вложения."));
                return Action::Continue;
            }
            attachment.description = Some(String::new());
            drop(messages);
            let _ = self.push_frame("div", FrameKind::AttachmentDescription);
            return Action::Continue;
        }
        if let Some(raw) = raw_kind(name) {
            if self.push_frame(name, FrameKind::Skip) {
                return Action::Raw(raw);
            }
            return Action::Continue;
        }
        self.fail(format!(
            "{ERR_STRUCTURE} Неподдерживаемый элемент <{name}> во вложении."
        ));
        Action::Continue
    }

    fn start_in_forward(&self, tag: &Tag) -> Action {
        let name = tag_name(tag);
        if name == "div" {
            if has_class(tag, "message") {
                self.fail(format!(
                    "{ERR_STRUCTURE} Вложенная пересылка внутри пересланного контекста не поддерживается."
                ));
                return Action::Continue;
            }
            let (header_seen, body_seen) = match self.messages.borrow().last() {
                Some(message) => match message.forwards.last() {
                    Some(context) => (context.header_seen, context.body_seen),
                    None => (false, false),
                },
                None => (false, false),
            };
            if has_class(tag, "message__header") {
                if header_seen {
                    self.fail(format!(
                        "{ERR_STRUCTURE} Повторный заголовок пересланного сообщения."
                    ));
                    return Action::Continue;
                }
                if self.push_frame("div", FrameKind::ForwardHeader) {
                    self.current_forward(|context| context.header_seen = true);
                }
                return Action::Continue;
            }
            if !header_seen {
                self.fail(format!(
                    "{ERR_STRUCTURE} Пересланное сообщение без заголовка не поддерживается."
                ));
                return Action::Continue;
            }
            if body_seen {
                self.fail(format!(
                    "{ERR_STRUCTURE} Неподдерживаемый элемент в пересланном сообщении после тела."
                ));
                return Action::Continue;
            }
            if self.push_frame("div", FrameKind::ForwardBody) {
                self.current_forward(|context| context.body_seen = true);
            }
            return Action::Continue;
        }
        if let Some(raw) = raw_kind(name) {
            if self.push_frame(name, FrameKind::Skip) {
                return Action::Raw(raw);
            }
            return Action::Continue;
        }
        self.fail(format!(
            "{ERR_STRUCTURE} Неподдерживаемый элемент <{name}> в пересланном сообщении."
        ));
        Action::Continue
    }

    fn start_in_forward_header(&self, tag: &Tag) -> Action {
        let name = tag_name(tag);
        if name == "a" {
            let author_seen = self
                .messages
                .borrow()
                .last()
                .and_then(|message| message.forwards.last())
                .is_some_and(|context| context.author_seen);
            if author_seen {
                self.fail(format!(
                    "{ERR_STRUCTURE} Повторная ссылка автора в пересланном сообщении."
                ));
                return Action::Continue;
            }
            let href = match attr(tag, "href") {
                None => None,
                Some(value)
                    if !value.trim().is_empty()
                        && value.len() <= MAX_AUTHOR_URL_BYTES
                        && !value.chars().any(char::is_control) =>
                {
                    Some(value.to_string())
                }
                Some(_) => {
                    self.fail(format!(
                        "{ERR_STRUCTURE} Некорректная ссылка автора пересылки."
                    ));
                    return Action::Continue;
                }
            };
            if self.push_frame("a", FrameKind::ForwardAuthorLink) {
                self.current_forward(|context| {
                    context.author_seen = true;
                    context.author_url = href;
                });
            }
            return Action::Continue;
        }
        if let Some(raw) = raw_kind(name) {
            if self.push_frame(name, FrameKind::Skip) {
                return Action::Raw(raw);
            }
            return Action::Continue;
        }
        self.fail(format!(
            "{ERR_STRUCTURE} Неподдерживаемый элемент <{name}> в заголовке пересланного сообщения."
        ));
        Action::Continue
    }

    // ---- end tags ----------------------------------------------------------
}
