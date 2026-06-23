//! Internationalization for CodeI.

mod error;

use error::I18nError;

rust_i18n::i18n!("locales", fallback = "en-US");

/// Supported UI languages.
pub const SUPPORTED_LOCALES: &[&str] = &["zh-CN", "en-US"];

/// Sets the active locale. Accepts `zh-CN` or `en-US`.
pub fn set_locale(language: &str) -> Result<(), I18nError> {
    if !SUPPORTED_LOCALES.contains(&language) {
        return Err(I18nError::UnsupportedLocale {
            locale: language.to_string(),
        });
    }
    rust_i18n::set_locale(language);
    Ok(())
}

/// Initializes i18n from a language tag, falling back to `en-US` when unknown.
pub fn init(language: &str) -> String {
    if set_locale(language).is_ok() {
        language.to_string()
    } else {
        rust_i18n::set_locale("en-US");
        "en-US".to_string()
    }
}

/// Returns the translated message for `key`.
pub fn t(key: &str) -> String {
    crate::_rust_i18n_translate(&rust_i18n::locale(), key).into_owned()
}

/// Returns the translated message with interpolation variables.
///
/// Example: `t_fmt("greeting", &[("name", "CodeI")])`
pub fn t_fmt(key: &str, args: &[(&str, &str)]) -> String {
    let mut message = t(key);
    for (name, value) in args {
        message = message.replace(&format!("{{{name}}}"), value);
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_chinese_locale() {
        set_locale("zh-CN").unwrap();
        assert!(t("greeting").contains("你好"));
    }

    #[test]
    fn loads_english_locale() {
        set_locale("en-US").unwrap();
        assert!(t("greeting").contains("Hello"));
    }

    #[test]
    fn resolves_dynamic_keys() {
        set_locale("zh-CN").unwrap();
        assert!(t("tui_input_normal").contains("输入"));
        set_locale("en-US").unwrap();
        assert!(t("tui_input_normal").contains("Input"));
    }
}
