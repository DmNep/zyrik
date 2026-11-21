//! Языки интерфейса (FR-017). Таблицы вшиты при сборке: с диска ничего не читается.

pub struct Locale {
    pub code: &'static str,
    pub strings: &'static [&'static str],
}

include!(concat!(env!("OUT_DIR"), "/i18n.rs"));

/// Язык — индекс в `LOCALES`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lang(usize);

impl Lang {
    /// `ru`, `RU`, `ru-RU`, `ru_RU.UTF-8` → ru.
    pub fn from_code(code: &str) -> Option<Lang> {
        let base = code
            .split(['-', '_', '.'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        LOCALES.iter().position(|l| l.code == base).map(Lang)
    }

    pub fn english() -> Lang {
        Lang::from_code("en").expect("en.toml есть всегда — проверяет build.rs")
    }

    pub fn code(self) -> &'static str {
        LOCALES[self.0].code
    }

    pub fn all() -> impl Iterator<Item = Lang> {
        (0..LOCALES.len()).map(Lang)
    }

    /// `en|ru` — для справки и сообщений об ошибке.
    pub fn codes() -> String {
        LOCALES.iter().map(|l| l.code).collect::<Vec<_>>().join("|")
    }

    pub fn tr(self, msg: Msg) -> &'static str {
        LOCALES[self.0].strings[msg as usize]
    }

    /// Строка с подстановками `{имя}`. Неизвестные подстановки остаются как есть.
    pub fn fmt(self, msg: Msg, args: &[(&str, &str)]) -> String {
        let mut s = self.tr(msg).to_string();
        for (name, value) in args {
            s = s.replace(&format!("{{{name}}}"), value);
        }
        s
    }
}

/// `--lang` → язык системы → английский (FR-017a).
pub fn choose(cli: Option<&str>, system: Option<&str>) -> Lang {
    cli.and_then(Lang::from_code)
        .or_else(|| system.and_then(Lang::from_code))
        .unwrap_or_else(Lang::english)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_has_every_string() {
        assert!(LOCALES.len() >= 2);
        for l in LOCALES {
            assert_eq!(l.strings.len(), MSG_COUNT, "{}", l.code);
            for (k, s) in MSG_KEYS.iter().zip(l.strings) {
                assert!(!s.trim().is_empty(), "{}: {k}", l.code);
            }
        }
        assert_eq!(ALL_MSGS.len(), MSG_COUNT);
    }

    #[test]
    fn chooses_language() {
        let ru = Lang::from_code("ru").unwrap();
        let en = Lang::english();
        assert_eq!(choose(Some("ru"), Some("en-US")), ru);
        assert_eq!(choose(None, Some("ru-RU")), ru);
        assert_eq!(choose(None, Some("de-DE")), en);
        assert_eq!(choose(None, None), en);
        assert_eq!(choose(Some("xx"), Some("ru")), ru);
        assert_eq!(Lang::from_code("RU_ru.UTF-8"), Some(ru));
    }

    #[test]
    fn substitutes() {
        let en = Lang::english();
        assert_eq!(en.fmt(Msg::ErrDuplicate, &[("option", "--key")]), "--key is given twice");
        let ru = Lang::from_code("ru").unwrap();
        assert_eq!(ru.tr(Msg::TrayCapture), "Зырнуть");
        assert!(ru.tr(Msg::HelpText).contains(r#"--save-dir "D:\Снимки""#));
    }
}
