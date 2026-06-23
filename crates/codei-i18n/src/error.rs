use thiserror::Error;

#[derive(Debug, Error)]
pub enum I18nError {
    #[error("unsupported locale: {locale}")]
    UnsupportedLocale { locale: String },
}
