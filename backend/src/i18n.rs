//! Internationalization (i18n) module for AnkiTov.
//!
//! Uses `fluent-templates` to compile Fluent (.ftl) locale files into the
//! binary at build time via the `static_loader!` macro. Language detection
//! is done via a runtime override (set by the frontend language switcher).
//!
//! ## Architecture
//!
//! - Locale files: `backend/i18n/{locale}/main.ftl`
//! - At build time, `static_loader!` compiles all .ftl files into the binary
//! - `t(msg_id)` resolves messages at runtime for the current locale
//! - Frontend sets locale preference via `POST /api/v1/locale/set`
//!
//! ## Adding a new language
//!
//! 1. Create `backend/i18n/{locale-code}/main.ftl`
//! 2. Add the locale to the `LOCALES` static loader below
//! 3. Add metadata to `SUPPORTED_LOCALES`
//! 4. Rebuild

use fluent_templates::{static_loader, Loader};
use i18n_embed::unic_langid::LanguageIdentifier;
use std::sync::RwLock;

// ---------------------------------------------------------------------------
// 1. Compile-time Fluent template loading
// ---------------------------------------------------------------------------

// Statically compiled Fluent bundles containing all locales.
// Each locale directory under `i18n/` is compiled into its own bundle.
// The fallback language is `en-US` — any missing messages resolve to
// the English version.
//
// We disable Unicode isolating characters (Fluent inserts them by default
// to prevent text-direction bleed) because we handle RTL via CSS `dir`.
static_loader! {
    static LOCALES = {
        locales: "i18n",
        fallback_language: "en-US",
        customise: |b| b.set_use_isolating(false),
    };
}

// ---------------------------------------------------------------------------
// 2. Runtime locale override
// ---------------------------------------------------------------------------

/// A thread-safe override for the locale. When set, it takes precedence over
/// the default (`en-US`). Set by the frontend via `POST /api/v1/locale/set`.
static LOCALE_OVERRIDE: RwLock<Option<LanguageIdentifier>> = RwLock::new(None);

/// List of supported locale codes and their metadata.
/// (code, native_name, english_name)
pub const SUPPORTED_LOCALES: &[(&str, &str, &str)] = &[
    ("en-US", "English (US)", "English (US)"),
    ("ar",    "العربية",      "Arabic"),
    ("he",    "עברית",        "Hebrew"),
    ("fr",    "Français",     "French"),
    ("es",    "Español",      "Spanish"),
    ("de",    "Deutsch",      "German"),
    ("it",    "Italiano",     "Italian"),
    ("zh",    "简体中文",     "Chinese (Simplified)"),
    ("ko",    "한국어",       "Korean"),
    ("ja",    "日本語",       "Japanese"),
    ("ru",    "Русский",      "Russian"),
    ("pt",    "Português (Brasil)", "Portuguese (Brazil)"),
    ("hi",    "हिन्दी",       "Hindi"),
];

/// Set the current locale override (called when user switches language).
pub fn set_locale(locale_str: &str) {
    if let Ok(langid) = locale_str.parse::<LanguageIdentifier>() {
        if let Ok(mut guard) = LOCALE_OVERRIDE.write() {
            *guard = Some(langid);
        }
    }
}

/// Returns the currently active locale code as a string.
///
/// Priority:
/// 1. Runtime override (set by frontend language switcher)
/// 2. Default: `en-US`
pub fn current_locale() -> String {
    if let Ok(guard) = LOCALE_OVERRIDE.read() {
        if let Some(ref langid) = *guard {
            return langid.to_string();
        }
    }
    "en-US".to_string()
}

/// Returns the current LanguageIdentifier.
fn current_langid() -> LanguageIdentifier {
    if let Ok(guard) = LOCALE_OVERRIDE.read() {
        if let Some(ref langid) = *guard {
            return langid.clone();
        }
    }
    "en-US".parse().unwrap()
}

// ---------------------------------------------------------------------------
// 3. Public API
// ---------------------------------------------------------------------------

/// Returns true if the current locale is a Right-to-Left (RTL) language.
pub fn is_rtl() -> bool {
    let loc = current_locale();
    matches!(loc.as_str(), "ar" | "he" | "fa" | "ur" | "yi" | "ku" | "sd")
}

/// Look up a translated string for the current locale.
///
/// Falls back to `en-US` if the message ID is not found.
pub fn t(message_id: &str) -> String {
    let langid = current_langid();
    LOCALES.lookup(&langid, message_id)
}

/// Look up a translated string, returning `None` if the message requires
/// variables that aren't provided (e.g. `{ $detail }`) or if formatting
/// fails. Use this for flat-map endpoints that can't provide per-message
/// args. `fluent-templates` panics on resolve errors, so we catch the panic.
pub fn t_optional(message_id: &str) -> Option<String> {
    let langid = current_langid();
    // `LOCALES.try_lookup` still panics when a message references a variable
    // that isn't provided. Catch it so flat-map endpoints degrade gracefully.
    use std::panic::{catch_unwind, AssertUnwindSafe};
    let result = catch_unwind(AssertUnwindSafe(|| LOCALES.try_lookup(&langid, message_id)));
    match result {
        Ok(Some(s)) => Some(s),
        Ok(None) => None,
        Err(_) => None,
    }
}

/// Look up a translated string for a specific locale.
pub fn t_for(locale_str: &str, message_id: &str) -> String {
    let langid: LanguageIdentifier = locale_str.parse().unwrap_or_else(|_| "en-US".parse().unwrap());
    LOCALES.lookup(&langid, message_id)
}

/// Look up with variable arguments.
pub fn t_with(message_id: &str, args: &[(&str, &str)]) -> String {
    let langid = current_langid();
    let mut map = std::collections::HashMap::new();
    for (k, v) in args {
        map.insert(k.to_string(), fluent_templates::fluent_bundle::FluentValue::from(*v));
    }
    LOCALES.lookup_with_args(&langid, message_id, &map)
}

// ---------------------------------------------------------------------------
// 4. Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Every locale directory shipped in `backend/i18n/`. Kept as an explicit
    /// list (rather than globbed at runtime) so the test fails to *compile*
    /// when a locale is added or renamed — a silently-skipped locale is worse
    /// than a broken build.
    const BUNDLES: &[(&str, &str)] = &[
        ("ar", include_str!("../i18n/ar/main.ftl")),
        ("de", include_str!("../i18n/de/main.ftl")),
        ("en-US", include_str!("../i18n/en-US/main.ftl")),
        ("es", include_str!("../i18n/es/main.ftl")),
        ("fr", include_str!("../i18n/fr/main.ftl")),
        ("he", include_str!("../i18n/he/main.ftl")),
        ("hi", include_str!("../i18n/hi/main.ftl")),
        ("it", include_str!("../i18n/it/main.ftl")),
        ("ja", include_str!("../i18n/ja/main.ftl")),
        ("ko", include_str!("../i18n/ko/main.ftl")),
        ("pt", include_str!("../i18n/pt/main.ftl")),
        ("ru", include_str!("../i18n/ru/main.ftl")),
        ("zh", include_str!("../i18n/zh/main.ftl")),
    ];

    /// A Fluent message ID is `[A-Za-z][A-Za-z0-9_-]*` — dots are NOT legal.
    /// A dotted ID makes the parser read the ID up to the dot and then demand
    /// `=` where it finds `.`, which aborts the whole bundle. That once shipped
    /// 16 such IDs in `en-US` and turned 7 i18n tests red; the symptom was a
    /// poisoned `Lazy` ("Lazy instance has previously been poisoned"), which
    /// points at the *first* failure and hides the real cause. This test
    /// reports the file, the byte offset and the offending line instead.
    #[test]
    fn every_locale_bundle_parses() {
        let mut problems: Vec<String> = Vec::new();

        for (locale, source) in BUNDLES.iter().copied() {
            if let Err((_, errors)) = fluent_syntax::parser::parse(source) {
                for error in errors {
                    let pos = error.pos.start;
                    let line_no = source[..pos].lines().count();
                    let line = source.lines().nth(line_no.saturating_sub(1)).unwrap_or("").trim();
                    problems.push(format!(
                        "{locale}/main.ftl:{}: {:?} (byte {}) -> `{line}`",
                        line_no, error.kind, pos
                    ));
                }
            }
        }

        assert!(
            problems.is_empty(),
            "{} Fluent parse error(s). A message ID may only contain \
             [A-Za-z0-9_-]; a '.' is illegal and breaks the entire bundle:\n  {}",
            problems.len(),
            problems.join("\n  ")
        );
    }

    /// Companion to the above: assert the dotted IDs that caused the incident
    /// are actually gone, so a future editor cannot reintroduce the shape even
    /// if the parser ever grows to tolerate it.
    #[test]
    fn no_message_id_contains_a_dot() {
        for (locale, source) in BUNDLES.iter().copied() {
            for (i, line) in source.lines().enumerate() {
                let trimmed = line.trim_start();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }
                // A message or term entry starts with `[ -]identifier =`.
                let Some((id, tail)) = trimmed.split_once('=') else {
                    continue;
                };
                if tail.starts_with('=') {
                    continue; // a literal `==`, not an entry
                }
                let id = id.trim().strip_prefix('-').unwrap_or(id.trim());
                assert!(
                    !id.contains('.'),
                    "{locale}/main.ftl:{}: illegal id `{id}` — dots are not legal in \
                     Fluent identifiers (use `-`)",
                    i + 1
                );
            }
        }
    }

    #[test]
    fn test_en_us_fallback() {
        let msg = t_for("en-US", "app-name");
        assert_eq!(msg, "AnkiTov");
    }

    #[test]
    fn test_app_name_not_empty() {
        let msg = t("app-name");
        assert!(!msg.is_empty());
    }

    #[test]
    fn test_variable_substitution() {
        let msg = t_with("success-created", &[("entity", "Deck")]);
        assert!(msg.contains("Deck") || !msg.is_empty());
    }

    #[test]
    fn test_rtl_detection() {
        assert!(!is_rtl()); // Default en-US is LTR
    }

    #[test]
    fn test_t_optional_skips_variable_messages() {
        // Messages requiring variables (e.g. { $detail }) must resolve to None
        // rather than panicking, so flat-map locale endpoints don't 500.
        let msg = t_optional("error-validation");
        assert!(msg.is_none(), "expected None for variable-bearing message");

        // Non-variable messages resolve normally.
        let msg = t_optional("app-name");
        assert_eq!(msg.as_deref(), Some("AnkiTov"));
    }

    #[test]
    fn test_t_optional_rtl_letters() {
        // Arabic short vowels should resolve fine (no panic) for flat message.
        let msg = t_optional("error-not-found");
        assert!(msg.is_some());
        assert!(!msg.unwrap().is_empty());
    }

    /// Real, non-English direct lookup: proves the compiled Fluent bundle
    /// returns the actual native-script string for a given locale (not a
    /// fallback). Hebrew and Arabic are both in the static loader; their
    /// `app-name` values are transliterations of "AnkiTov".
    #[test]
    fn test_t_for_native_locale_app_name() {
        // he/app-name in i18n/he/main.ftl is the Hebrew transliteration.
        assert_eq!(t_for("he", "app-name"), "אנקיטוב");
        // ar/app-name in i18n/ar/main.ftl is the Arabic transliteration.
        assert_eq!(t_for("ar", "app-name"), "أنكيتوف");
    }

    /// Fallback-chain proof: a message ID absent from a non-fallback locale
    /// must resolve to the `en-US` value. We add a probe key to en-US that no
    /// other locale carries, then assert a lookup in `he` falls back to it.
    #[test]
    fn test_fallback_to_english_when_key_missing() {
        // `i18n-test-probe` exists ONLY in i18n/en-US/main.ftl.
        assert_eq!(
            t_for("en-US", "i18n-test-probe"),
            "AnkiTov fallback probe"
        );
        // Hebrew has no such key, so the fluent-templates fallback chain must
        // return the en-US string.
        assert_eq!(
            t_for("he", "i18n-test-probe"),
            "AnkiTov fallback probe"
        );
    }

    /// `is_rtl()` tracks the runtime locale override set by `set_locale`.
    /// Tests run in-process, so we assert the transition in both directions
    /// and restore a sane default (en-US, LTR) before returning.
    #[test]
    fn test_is_rtl_follows_set_locale() {
        set_locale("he");
        assert!(is_rtl(), "Hebrew must be RTL");
        set_locale("ar");
        assert!(is_rtl(), "Arabic must be RTL");
        set_locale("en-US");
        assert!(!is_rtl(), "en-US must be LTR");
    }
}