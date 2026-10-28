use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource, FluentValue};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};
use unic_langid::LanguageIdentifier;

const EN_FTL: &str = include_str!("../../../locales/en.ftl");
const TR_FTL: &str = include_str!("../../../locales/tr.ftl");

/// Supported languages in beaM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Language {
    #[default]
    En,
    Tr,
}

impl Language {
    /// 2-letter ISO 639-1 language code.
    pub fn code(&self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Tr => "tr",
        }
    }

    /// Native localized name of the language.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::En => "English",
            Self::Tr => "Türkçe",
        }
    }

    /// Parse language from locale code or name (e.g. "en", "en_US", "tr", "tr_TR").
    pub fn from_locale_str(s: &str) -> Self {
        let lower = s.to_ascii_lowercase();
        if lower.starts_with("tr") {
            Self::Tr
        } else {
            Self::En
        }
    }
}

/// Detect system language using `sys-locale`, falling back to English.
pub fn system_language() -> Language {
    sys_locale::get_locale()
        .map(|s| Language::from_locale_str(&s))
        .unwrap_or(Language::En)
}

/// Persistent application configuration (stores language preference).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub language: Language,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            language: system_language(),
        }
    }
}

impl AppConfig {
    /// Default user configuration path (`~/.config/beam/config.ron` or `%APPDATA%\beam\config.ron`).
    pub fn default_path() -> PathBuf {
        if let Ok(config_home) = std::env::var("XDG_CONFIG_HOME") {
            PathBuf::from(config_home).join("beam").join("config.ron")
        } else if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home).join(".config/beam/config.ron")
        } else if let Ok(appdata) = std::env::var("APPDATA") {
            PathBuf::from(appdata).join("beam/config.ron")
        } else {
            PathBuf::from("beam_config.ron")
        }
    }

    /// Load configuration from disk, falling back to system defaults.
    pub fn load() -> Self {
        let path = Self::default_path();
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(cfg) = ron::from_str::<AppConfig>(&content) {
                    return cfg;
                }
            }
        }
        Self::default()
    }

    /// Save configuration to disk.
    pub fn save(&self) -> Result<(), std::io::Error> {
        let path = Self::default_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let pretty = ron::ser::PrettyConfig::new();
        let s = ron::ser::to_string_pretty(self, pretty)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        std::fs::write(&path, s)
    }
}

/// Localization instance managing Fluent bundles.
pub struct I18n {
    current_lang: Language,
    bundle_en: FluentBundle<FluentResource>,
    bundle_tr: FluentBundle<FluentResource>,
}

impl I18n {
    pub fn new(initial_lang: Language) -> Self {
        let res_en =
            FluentResource::try_new(EN_FTL.to_string()).expect("Failed to parse locales/en.ftl");
        let lang_en: LanguageIdentifier = "en-US".parse().expect("Valid language identifier");
        let mut bundle_en = FluentBundle::new_concurrent(vec![lang_en]);
        bundle_en.set_use_isolating(false);
        bundle_en
            .add_resource(res_en)
            .expect("Failed to add en resource");

        let res_tr =
            FluentResource::try_new(TR_FTL.to_string()).expect("Failed to parse locales/tr.ftl");
        let lang_tr: LanguageIdentifier = "tr-TR".parse().expect("Valid language identifier");
        let mut bundle_tr = FluentBundle::new_concurrent(vec![lang_tr]);
        bundle_tr.set_use_isolating(false);
        bundle_tr
            .add_resource(res_tr)
            .expect("Failed to add tr resource");

        Self {
            current_lang: initial_lang,
            bundle_en,
            bundle_tr,
        }
    }

    pub fn language(&self) -> Language {
        self.current_lang
    }

    pub fn set_language(&mut self, lang: Language) {
        self.current_lang = lang;
    }

    /// Translate a message key into the currently active language.
    pub fn t(&self, key: &str) -> String {
        self.format_key(key, None)
    }

    /// Translate a message key with arguments into the currently active language.
    pub fn t_with_args(&self, key: &str, args: &[(&str, &str)]) -> String {
        let mut fluent_args = FluentArgs::new();
        for (k, v) in args {
            fluent_args.set(*k, FluentValue::from(*v));
        }
        self.format_key(key, Some(&fluent_args))
    }

    fn format_key(&self, key: &str, args: Option<&FluentArgs>) -> String {
        let primary_bundle = match self.current_lang {
            Language::En => &self.bundle_en,
            Language::Tr => &self.bundle_tr,
        };

        // Attempt lookup in current language bundle
        if let Some(msg) = primary_bundle.get_message(key) {
            if let Some(pattern) = msg.value() {
                let mut errors = vec![];
                let value = primary_bundle.format_pattern(pattern, args, &mut errors);
                return value.to_string();
            }
        }

        // Fallback to English bundle if not found in primary
        if self.current_lang != Language::En {
            if let Some(msg) = self.bundle_en.get_message(key) {
                if let Some(pattern) = msg.value() {
                    let mut errors = vec![];
                    let value = self.bundle_en.format_pattern(pattern, args, &mut errors);
                    return value.to_string();
                }
            }
        }

        // Return key if completely missing
        key.to_string()
    }
}

static GLOBAL_I18N: OnceLock<RwLock<I18n>> = OnceLock::new();

fn global_i18n() -> &'static RwLock<I18n> {
    GLOBAL_I18N.get_or_init(|| RwLock::new(I18n::new(Language::default())))
}

/// Set the global application language.
pub fn set_language(lang: Language) {
    if let Ok(mut lock) = global_i18n().write() {
        lock.set_language(lang);
    }
}

/// Get the current application language.
pub fn current_language() -> Language {
    global_i18n()
        .read()
        .map(|l| l.language())
        .unwrap_or_default()
}

/// Translate message by key in global active language.
pub fn t(key: &str) -> String {
    global_i18n()
        .read()
        .map(|l| l.t(key))
        .unwrap_or_else(|_| key.to_string())
}

/// Translate message with key and arguments in global active language.
pub fn t_with_args(key: &str, args: &[(&str, &str)]) -> String {
    global_i18n()
        .read()
        .map(|l| l.t_with_args(key, args))
        .unwrap_or_else(|_| key.to_string())
}

/// Convenient macro for translating strings.
///
/// Usage:
/// ```ignore
/// let title = t!("menu_file");
/// let count_text = t!("library_all_photos", "count" => 42);
/// ```
#[macro_export]
macro_rules! t {
    ($key:expr) => {
        $crate::i18n::t($key)
    };
    ($key:expr, $($arg_key:expr => $arg_val:expr),* $(,)?) => {
        $crate::i18n::t_with_args($key, &[ $( ($arg_key, &$arg_val.to_string()) ),* ])
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_translations_parity() {
        // Parse raw FTL files to ensure every message key exists in both English and Turkish
        fn extract_keys(ftl: &str) -> std::collections::BTreeSet<String> {
            let mut keys = std::collections::BTreeSet::new();
            for line in ftl.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }
                if let Some((key, _)) = trimmed.split_once('=') {
                    let k = key.trim().to_string();
                    if !k.is_empty() {
                        keys.insert(k);
                    }
                }
            }
            keys
        }

        let en_keys = extract_keys(EN_FTL);
        let tr_keys = extract_keys(TR_FTL);

        let missing_in_tr: Vec<_> = en_keys.difference(&tr_keys).collect();
        let missing_in_en: Vec<_> = tr_keys.difference(&en_keys).collect();

        assert!(
            missing_in_tr.is_empty(),
            "Keys present in en.ftl but missing in tr.ftl: {:?}",
            missing_in_tr
        );
        assert!(
            missing_in_en.is_empty(),
            "Keys present in tr.ftl but missing in en.ftl: {:?}",
            missing_in_en
        );
        assert!(!en_keys.is_empty(), "en.ftl has no keys parsed!");
    }

    #[test]
    fn test_translation_formatting_and_switching() {
        let mut i18n = I18n::new(Language::En);
        assert_eq!(i18n.t("develop_exposure"), "Exposure");
        assert_eq!(
            i18n.t_with_args("library_all_photos", &[("count", "15")]),
            "All Photos (15)"
        );

        i18n.set_language(Language::Tr);
        assert_eq!(i18n.t("develop_exposure"), "Pozlama");
        assert_eq!(
            i18n.t_with_args("library_all_photos", &[("count", "15")]),
            "Tüm Fotoğraflar (15)"
        );
    }

    #[test]
    fn test_language_detection_and_fallback() {
        assert_eq!(Language::from_locale_str("tr_TR"), Language::Tr);
        assert_eq!(Language::from_locale_str("tr"), Language::Tr);
        assert_eq!(Language::from_locale_str("en_US"), Language::En);
        assert_eq!(Language::from_locale_str("fr_FR"), Language::En);
    }

    #[test]
    fn test_app_config_roundtrip() {
        let temp_dir = std::env::temp_dir().join("beam_test_config");
        let _ = std::fs::create_dir_all(&temp_dir);
        let cfg_path = temp_dir.join("config.ron");

        let cfg = AppConfig {
            language: Language::Tr,
        };
        let s = ron::ser::to_string_pretty(&cfg, ron::ser::PrettyConfig::new()).unwrap();
        std::fs::write(&cfg_path, &s).unwrap();

        let loaded: AppConfig =
            ron::from_str(&std::fs::read_to_string(&cfg_path).unwrap()).unwrap();
        assert_eq!(loaded.language, Language::Tr);

        let _ = std::fs::remove_dir_all(temp_dir);
    }
}
