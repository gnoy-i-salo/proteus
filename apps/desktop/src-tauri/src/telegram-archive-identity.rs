//! Archive identities are versioned separately from external Telegram fields.
//! Exported bare IDs are not unique across user/basic-chat/channel namespaces.
use super::telegram_archive_json::TelegramChatHeader;
use sha2::{Digest, Sha256};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArchiveVersion {
    V1,
    V2,
}
impl ArchiveVersion {
    pub const CURRENT: Self = Self::V2;
    pub fn from_adapter(value: &str) -> Result<Self, String> {
        match value {
            "telegramDesktopJson:1" => Ok(Self::V1),
            "telegramDesktopJson:2" => Ok(Self::V2),
            _ => Err("Неподдерживаемая версия адаптера архива Telegram.".into()),
        }
    }
    pub fn adapter(self) -> &'static str {
        match self {
            Self::V1 => "telegramDesktopJson:1",
            Self::V2 => "telegramDesktopJson:2",
        }
    }
    pub fn document_version(self) -> u8 {
        match self {
            Self::V1 => 1,
            Self::V2 => 2,
        }
    }
    pub fn batch_codec(self) -> &'static str {
        match self {
            Self::V1 => "telegramArchiveBatch:1",
            Self::V2 => "telegramArchiveBatch:2",
        }
    }
    pub fn resource_id(self, chat: &TelegramChatHeader) -> String {
        if self == Self::V1 {
            return format!("telegram:chat:{}", chat.id);
        }
        let class =
            match chat.kind.as_str() {
                "saved_messages" | "replies" | "verification_codes" | "personal_chat"
                | "bot_chat" => "user",
                "private_group" => "chat",
                "private_supergroup" | "public_supergroup" | "private_channel"
                | "public_channel" => "channel",
                // Preserve unknown kinds without guessing a peer type or merging
                // distinct future formats. Empty/unknown dialogs remain selectable.
                other => {
                    return format!(
                        "telegram:peer:unknown:{:x}:{}",
                        Sha256::digest(other.as_bytes()),
                        chat.id
                    )
                }
            };
        format!("telegram:peer:{class}:{}", chat.id)
    }
    pub fn identifiers(
        self,
        origin: &str,
        resource: &str,
        original: &str,
    ) -> Result<(String, String), String> {
        let bytes = serde_json::to_vec(&(origin, resource, original))
            .map_err(|_| "Не удалось определить источник сообщения.")?;
        let hash = format!("{:x}", Sha256::digest(bytes));
        Ok((
            format!("telegramArchive:{}:{hash}", self.document_version()),
            format!("document:{hash}"),
        ))
    }
}
