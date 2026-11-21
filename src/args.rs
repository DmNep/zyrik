//! Параметры запуска (история 8). Файла настроек нет: всё живёт в ярлыке.
//! Разбор вручную, без сторонних crate'ов.

use std::path::PathBuf;

use crate::hotkey::{Hotkey, KeyError};
use crate::i18n::Lang;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// Захват области
    pub key: Hotkey,
    /// Захват всего монитора
    pub key_screen: Option<Hotkey>,
    /// Захват активного окна
    pub key_window: Option<Hotkey>,
    /// Начальная папка для Ctrl+S; `None` — «Изображения»
    pub save_dir: Option<PathBuf>,
    /// Через сколько минут очистить буфер, если там всё ещё наш снимок
    pub clear_clipboard: Option<u32>,
    /// Искать «опасное» на снимке
    pub detect: bool,
    /// Код языка из `--lang`, уже проверенный
    pub lang: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Launch {
    Run(Options),
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgError {
    Unknown(String),
    MissingValue(&'static str),
    Duplicate(&'static str),
    BadKey { option: &'static str, value: String, reason: KeyError },
    BadMinutes(String),
    BadLang(String),
    /// Одна и та же комбинация у двух параметров
    SameKey(&'static str, &'static str),
}

pub const MAX_CLEAR_MINUTES: u32 = 24 * 60;

const KEY: &str = "--key";
const KEY_SCREEN: &str = "--key-screen";
const KEY_WINDOW: &str = "--key-window";
const SAVE_DIR: &str = "--save-dir";
const CLEAR: &str = "--clear-clipboard";
const NO_DETECT: &str = "--no-detect";
const LANG: &str = "--lang";

const WITH_VALUE: [&str; 6] = [KEY, KEY_SCREEN, KEY_WINDOW, SAVE_DIR, CLEAR, LANG];

fn is_help(arg: &str) -> bool {
    matches!(arg, "--help" | "-h" | "/?" | "help")
}

pub fn parse<I, S>(args: I) -> Result<Launch, ArgError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let args: Vec<String> = args.into_iter().map(|a| a.as_ref().to_string()).collect();
    if args.iter().any(|a| is_help(a)) {
        return Ok(Launch::Help);
    }

    let mut key = None;
    let mut key_screen = None;
    let mut key_window = None;
    let mut save_dir = None;
    let mut clear_clipboard = None;
    let mut no_detect = false;
    let mut lang = None;

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        if arg == NO_DETECT {
            if no_detect {
                return Err(ArgError::Duplicate(NO_DETECT));
            }
            no_detect = true;
            continue;
        }
        let (name, inline) = match arg.split_once('=') {
            Some((n, v)) => (n, Some(v.to_string())),
            None => (arg.as_str(), None),
        };
        let Some(&option) = WITH_VALUE.iter().find(|o| **o == name) else {
            return Err(ArgError::Unknown(arg.clone()));
        };
        let value = match inline {
            Some(v) => v,
            None => match it.next() {
                Some(v) if !v.starts_with("--") => v.clone(),
                _ => return Err(ArgError::MissingValue(option)),
            },
        };
        if value.is_empty() {
            return Err(ArgError::MissingValue(option));
        }
        let duplicate = match option {
            KEY => key.is_some(),
            KEY_SCREEN => key_screen.is_some(),
            KEY_WINDOW => key_window.is_some(),
            SAVE_DIR => save_dir.is_some(),
            CLEAR => clear_clipboard.is_some(),
            _ => lang.is_some(),
        };
        if duplicate {
            return Err(ArgError::Duplicate(option));
        }
        match option {
            KEY | KEY_SCREEN | KEY_WINDOW => {
                let hk = Hotkey::parse(&value).map_err(|reason| ArgError::BadKey {
                    option,
                    value: value.clone(),
                    reason,
                })?;
                match option {
                    KEY => key = Some(hk),
                    KEY_SCREEN => key_screen = Some(hk),
                    _ => key_window = Some(hk),
                }
            }
            SAVE_DIR => save_dir = Some(PathBuf::from(value)),
            CLEAR => {
                let ok = value.bytes().all(|b| b.is_ascii_digit());
                match value.parse::<u32>() {
                    Ok(n) if ok && (1..=MAX_CLEAR_MINUTES).contains(&n) => clear_clipboard = Some(n),
                    _ => return Err(ArgError::BadMinutes(value)),
                }
            }
            _ => match Lang::from_code(&value) {
                Some(l) => lang = Some(l.code().to_string()),
                None => return Err(ArgError::BadLang(value)),
            },
        }
    }

    let key = key.unwrap_or_else(Hotkey::default_capture);
    let named = [(KEY, Some(key)), (KEY_SCREEN, key_screen), (KEY_WINDOW, key_window)];
    for (i, (a, ka)) in named.iter().enumerate() {
        for (b, kb) in &named[i + 1..] {
            if ka.is_some() && ka == kb {
                return Err(ArgError::SameKey(a, b));
            }
        }
    }

    Ok(Launch::Run(Options {
        key,
        key_screen,
        key_window,
        save_dir,
        clear_clipboard,
        detect: !no_detect,
        lang,
    }))
}

/// Язык из `--lang`, даже если остальные параметры с ошибкой: сообщение об ошибке
/// должно быть на том языке, который человек просил.
pub fn lang_hint<I, S>(args: I) -> Option<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let args: Vec<String> = args.into_iter().map(|a| a.as_ref().to_string()).collect();
    let mut found = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let value = if a == LANG {
            it.next().cloned()
        } else {
            a.strip_prefix("--lang=").map(str::to_string)
        };
        if let Some(l) = value.as_deref().and_then(Lang::from_code) {
            found = Some(l.code().to_string());
        }
    }
    found
}

/// Путь к запущенному файлу — для примеров в справке.
pub fn program_path() -> String {
    let shown = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "zyrik".to_string());
    shown.strip_prefix(r"\\?\").unwrap_or(&shown).to_string()
}
