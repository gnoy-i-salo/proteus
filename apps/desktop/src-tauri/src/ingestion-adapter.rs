//! Native adapter identity comes from a verified preview, never a frontend flag.
use super::{
    page_document::PageOrigin, preview_store::StoredPreview, telegram_identity::ArchiveVersion,
};
use crate::corpus_database::JobProvider;
#[derive(Clone, Copy)]
pub enum Adapter {
    Telegram,
    SavedPage,
    SelectedUrl,
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
            "web"
                if preview.schema_version == 4
                    && preview.adapter_version == super::page_preview::URL_ADAPTER_VERSION
                    && matches!(preview.origin, Some(PageOrigin::SelectedUrl { .. })) =>
            {
                content_type(preview)?;
                Ok(Self::SelectedUrl)
            }
            _ => Err("Этот формат ещё не поддерживает потоковый импорт.".into()),
        }
    }
    pub fn provider(self) -> JobProvider {
        match self {
            Self::Telegram => JobProvider::Telegram,
            Self::SavedPage => JobProvider::SavedPage,
            Self::SelectedUrl => JobProvider::Web,
        }
    }
}
pub(super) fn content_type(preview: &StoredPreview) -> Result<&'static str, String> {
    match (preview.provider.as_str(), preview.source_format.as_str()) {
        ("savedPage", "savedPageHtml") | ("web", "selectedUrlHtml") => Ok("text/html"),
        ("savedPage", "savedPageText") | ("web", "selectedUrlText") => Ok("text/plain"),
        ("savedPage", "savedPageMarkdown") | ("web", "selectedUrlMarkdown") => Ok("text/markdown"),
        _ => Err("Неподдерживаемый формат сохранённой страницы.".into()),
    }
}

pub(super) enum CompletedAdapter {
    Telegram {
        version: ArchiveVersion,
        origin: String,
    },
    Page(Box<StoredPreview>),
}
impl CompletedAdapter {
    pub fn resolve(
        job: &crate::corpus_database::StoredJob,
        preview: &StoredPreview,
        provider: JobProvider,
        now: i64,
    ) -> Result<Self, String> {
        let adapter = Adapter::resolve(preview)?;
        if adapter.provider() != provider
            || job.state.checkpoint.adapter_version != preview.adapter_version
        {
            return Err("Источник принадлежит другому адаптеру.".into());
        }
        match adapter {
            Adapter::Telegram => Ok(Self::Telegram {
                version: ArchiveVersion::from_adapter(&preview.adapter_version)?,
                origin: super::telegram_worker::completed_origin(job, &preview.fingerprint, now)?,
            }),
            Adapter::SavedPage | Adapter::SelectedUrl => {
                super::page_worker::completed(job, preview, now)?;
                Ok(Self::Page(Box::new(preview.clone())))
            }
        }
    }
    pub fn preparation_version(&self) -> &'static str {
        match self {
            Self::Telegram { .. } => "telegramPreparation:1",
            Self::Page(preview) if preview.provider == "web" => "selectedUrlPreparation:1",
            Self::Page(_) => "savedPagePreparation:1",
        }
    }
    pub fn selected_timestamp(
        &self,
        revision: &crate::corpus_database::StoredRevision,
        binding: &crate::corpus_database::JobBinding,
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<Option<i64>, String> {
        check()?;
        let (selected, created) = match self {
            Self::Telegram { version, origin } => {
                let doc = super::telegram_document::decode_revision(*version, origin, revision)?;
                (
                    super::telegram_worker::selected(std::slice::from_ref(binding), &doc)?,
                    doc.created_at,
                )
            }
            Self::Page(preview) => {
                let doc = super::page_source::decode_revision(preview, revision, check)?;
                (
                    super::page_source::selected(std::slice::from_ref(binding), &doc)?,
                    doc.published_at,
                )
            }
        };
        check()?;
        if !selected {
            return Err("Фрагмент вне разрешённой области источника.".into());
        }
        created.as_deref().map(super::period::date_ms).transpose()
    }
}
