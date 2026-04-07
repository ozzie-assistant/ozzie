use std::collections::HashMap;
use std::sync::RwLock;

static CATALOGS: std::sync::LazyLock<RwLock<HashMap<String, HashMap<String, String>>>> =
    std::sync::LazyLock::new(|| RwLock::new(HashMap::new()));

static LANG: std::sync::LazyLock<RwLock<String>> =
    std::sync::LazyLock::new(|| RwLock::new(detect()));

/// Registers translation entries for a language.
pub fn register(lang: &str, entries: HashMap<String, String>) {
    let mut catalogs = CATALOGS.write().unwrap_or_else(|e| e.into_inner());
    let catalog = catalogs.entry(lang.to_string()).or_default();
    catalog.extend(entries);
}

/// Translates a key using the current language.
///
/// Fallback chain: current language → "en" → key itself.
pub fn t(key: &str) -> String {
    let lang = current_lang();
    let catalogs = CATALOGS.read().unwrap_or_else(|e| e.into_inner());

    // Try current language
    if let Some(catalog) = catalogs.get(&lang)
        && let Some(val) = catalog.get(key)
    {
        return val.clone();
    }

    // Fallback to English
    if lang != "en"
        && let Some(catalog) = catalogs.get("en")
        && let Some(val) = catalog.get(key)
    {
        return val.clone();
    }

    // Return key itself
    key.to_string()
}

/// Sets the active language.
pub fn set_lang(lang: &str) {
    *LANG.write().unwrap_or_else(|e| e.into_inner()) = lang.to_string();
}

/// Returns the current active language.
pub fn current_lang() -> String {
    LANG.read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Returns all keys registered for a language, sorted.
pub fn keys(lang: &str) -> Vec<String> {
    let catalogs = CATALOGS.read().unwrap_or_else(|e| e.into_inner());
    let mut result: Vec<String> = catalogs
        .get(lang)
        .map(|c| c.keys().cloned().collect())
        .unwrap_or_default();
    result.sort();
    result
}

/// Returns all supported locale codes, sorted.
pub fn supported_locales() -> Vec<String> {
    let catalogs = CATALOGS.read().unwrap_or_else(|e| e.into_inner());
    let mut locales: Vec<String> = catalogs.keys().cloned().collect();
    locales.sort();
    locales
}

/// Detects the language from environment variables.
///
/// Checks `LC_ALL` then `LANG`, extracts the 2-letter language code.
/// Falls back to "en".
pub fn detect() -> String {
    for var in &["LC_ALL", "LANG"] {
        if let Ok(val) = std::env::var(var) {
            let lang = extract_lang(&val);
            if !lang.is_empty() {
                return lang;
            }
        }
    }
    "en".to_string()
}

/// Extracts a 2-letter language code from a locale string.
///
/// Examples: "fr_FR.UTF-8" → "fr", "en_US" → "en", "C" → ""
fn extract_lang(locale: &str) -> String {
    let s = locale.split('.').next().unwrap_or("");
    let s = s.split('_').next().unwrap_or("");
    if s.len() == 2 && s.chars().all(|c| c.is_ascii_lowercase()) {
        s.to_string()
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Serialize tests that touch global state
    static TEST_LOCK: std::sync::LazyLock<Mutex<()>> = std::sync::LazyLock::new(|| Mutex::new(()));

    fn setup() -> std::sync::MutexGuard<'static, ()> {
        let guard = TEST_LOCK.lock().unwrap();
        let mut catalogs = CATALOGS.write().unwrap();
        catalogs.clear();
        drop(catalogs);
        set_lang("en");
        guard
    }

    #[test]
    fn register_and_translate() {
        let _g = setup();
        let mut entries = HashMap::new();
        entries.insert("greeting".to_string(), "Hello".to_string());
        register("en", entries);

        assert_eq!(t("greeting"), "Hello");
    }

    #[test]
    fn fallback_to_english() {
        let _g = setup();
        let mut en = HashMap::new();
        en.insert("greeting".to_string(), "Hello".to_string());
        register("en", en);

        set_lang("fr");
        assert_eq!(t("greeting"), "Hello");
    }

    #[test]
    fn fallback_to_key() {
        let _g = setup();
        assert_eq!(t("unknown_key"), "unknown_key");
    }

    #[test]
    fn language_override() {
        let _g = setup();
        let mut en = HashMap::new();
        en.insert("greeting".to_string(), "Hello".to_string());
        register("en", en);

        let mut fr = HashMap::new();
        fr.insert("greeting".to_string(), "Bonjour".to_string());
        register("fr", fr);

        set_lang("fr");
        assert_eq!(t("greeting"), "Bonjour");

        set_lang("en");
        assert_eq!(t("greeting"), "Hello");
    }

    #[test]
    fn keys_sorted() {
        let _g = setup();
        let mut entries = HashMap::new();
        entries.insert("b_key".to_string(), "B".to_string());
        entries.insert("a_key".to_string(), "A".to_string());
        register("en", entries);

        assert_eq!(keys("en"), vec!["a_key", "b_key"]);
    }

    #[test]
    fn supported_locales_list() {
        let _g = setup();
        register("fr", HashMap::new());
        register("en", HashMap::new());
        register("de", HashMap::new());

        let locales = supported_locales();
        assert_eq!(locales, vec!["de", "en", "fr"]);
    }

    #[test]
    fn extract_lang_variants() {
        // Pure function, no global state
        assert_eq!(extract_lang("fr_FR.UTF-8"), "fr");
        assert_eq!(extract_lang("en_US"), "en");
        assert_eq!(extract_lang("de"), "de");
        assert_eq!(extract_lang("C"), "");
        assert_eq!(extract_lang(""), "");
        assert_eq!(extract_lang("POSIX"), "");
    }

    #[test]
    fn merge_entries() {
        let _g = setup();
        let mut e1 = HashMap::new();
        e1.insert("a".to_string(), "A".to_string());
        register("en", e1);

        let mut e2 = HashMap::new();
        e2.insert("b".to_string(), "B".to_string());
        register("en", e2);

        assert_eq!(t("a"), "A");
        assert_eq!(t("b"), "B");
    }
}
