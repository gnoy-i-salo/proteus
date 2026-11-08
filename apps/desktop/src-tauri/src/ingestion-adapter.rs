//! Native adapter identity comes from a verified preview, never a frontend flag.
use super::{
    page_document::PageOrigin, preview_store::StoredPreview, telegram_identity::ArchiveVersion,
};
use crate::corpus_database::JobProvider;
#[derive(Clone, Copy)]
pub enum Adapter {
    Telegram,
    SavedPage,
}
impl Adapter {
    pub fn resolve(preview: &StoredPreview) -> Result<Self, String> {
        match preview.provider.as_str() {
            "telegram" if preview.source_format == "telegramDesktopJson" => {
                ArchiveVersion::from_adapter(&preview.adapter_version)?;
                Ok(Self::Telegram)
            }
            "savedPage"
                if preview.schema_version == 3
                    && preview.adapter_version == super::page_preview::ADAPTER_VERSION
                    && matches!(preview.origin, Some(PageOrigin::SavedPage { .. })) =>
            {
                content_type(preview)?;
                Ok(Self::SavedPage)
            }
            _ => Err("Этот формат ещё не поддерживает потоковый импорт.".into()),
        }
    }
    pub fn provider(self) -> JobProvider {
        match self {
            Self::Telegram => JobProvider::Telegram,
            Self::SavedPage => JobProvider::SavedPage,
        }
    }
}
pub(super) fn content_type(preview: &StoredPreview) -> Result<&'static str, String> {
    match preview.source_format.as_str() {
        "savedPageHtml" => Ok("text/html"),
        "savedPageText" => Ok("text/plain"),
        "savedPageMarkdown" => Ok("text/markdown"),
        _ => Err("Неподдерживаемый формат сохранённой страницы.".into()),
    }
}
