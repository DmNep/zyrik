//! Комбинация клавиш в виде, не зависящем от ОС: `Ctrl+Shift+PrtScr`.
//! Перевод в коды конкретной ОС — в прослойке платформы.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    PrintScreen,
    Pause,
    ScrollLock,
    Insert,
    Delete,
    Home,
    End,
    PageUp,
    PageDown,
    /// F1…F24
    F(u8),
    /// A…Z, всегда заглавная
    Letter(char),
    /// 0…9
    Digit(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    /// Win на Windows, Cmd на macOS, Super на Linux
    pub meta: bool,
}

impl Mods {
    pub fn any(self) -> bool {
        self.ctrl || self.shift || self.alt || self.meta
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hotkey {
    pub mods: Mods,
    pub key: Key,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyError {
    Empty,
    /// Непонятная часть комбинации
    Unknown(String),
    /// Одни модификаторы, клавиши нет
    NoKey,
    /// Две обычные клавиши в одной комбинации
    TwoKeys,
    /// Модификатор указан дважды
    Repeated(String),
    /// Буква или цифра без модификатора перехватила бы обычный набор текста
    NeedsModifier,
}

/// Название клавиши Win/Cmd/Super на текущей ОС.
pub const META_NAME: &str = if cfg!(target_os = "macos") {
    "Cmd"
} else if cfg!(windows) {
    "Win"
} else {
    "Super"
};

impl Hotkey {
    pub const fn new(mods: Mods, key: Key) -> Self {
        Hotkey { mods, key }
    }

    /// Клавиша по умолчанию для захвата области (история 8).
    pub fn default_capture() -> Self {
        if cfg!(target_os = "macos") {
            Hotkey::new(Mods { meta: true, shift: true, ..Mods::default() }, Key::Digit(2))
        } else {
            Hotkey::new(Mods { ctrl: true, ..Mods::default() }, Key::PrintScreen)
        }
    }

    pub fn parse(text: &str) -> Result<Self, KeyError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(KeyError::Empty);
        }
        let mut mods = Mods::default();
        let mut key = None;
        for raw in text.split('+') {
            let part = raw.trim();
            if part.is_empty() {
                return Err(KeyError::Unknown(raw.to_string()));
            }
            let lower = part.to_ascii_lowercase();
            let flag = match lower.as_str() {
                "ctrl" | "control" | "ctl" => Some(&mut mods.ctrl),
                "shift" => Some(&mut mods.shift),
                "alt" | "option" | "opt" => Some(&mut mods.alt),
                "win" | "cmd" | "command" | "super" | "meta" => Some(&mut mods.meta),
                _ => None,
            };
            if let Some(flag) = flag {
                if *flag {
                    return Err(KeyError::Repeated(part.to_string()));
                }
                *flag = true;
                continue;
            }
            let k = parse_key(&lower).ok_or_else(|| KeyError::Unknown(part.to_string()))?;
            if key.replace(k).is_some() {
                return Err(KeyError::TwoKeys);
            }
        }
        let key = key.ok_or(KeyError::NoKey)?;
        if matches!(key, Key::Letter(_) | Key::Digit(_)) && !mods.any() {
            return Err(KeyError::NeedsModifier);
        }
        Ok(Hotkey { mods, key })
    }
}

fn parse_key(lower: &str) -> Option<Key> {
    Some(match lower {
        "prtscr" | "prtsc" | "printscreen" | "print" | "prt" | "snapshot" => Key::PrintScreen,
        "pause" | "break" => Key::Pause,
        "scrolllock" | "scroll" => Key::ScrollLock,
        "insert" | "ins" => Key::Insert,
        "delete" | "del" => Key::Delete,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" | "pgup" => Key::PageUp,
        "pagedown" | "pgdn" => Key::PageDown,
        _ => {
            let mut chars = lower.chars();
            let first = chars.next()?;
            let rest = chars.as_str();
            if rest.is_empty() {
                return match first {
                    'a'..='z' => Some(Key::Letter(first.to_ascii_uppercase())),
                    '0'..='9' => Some(Key::Digit(first as u8 - b'0')),
                    _ => None,
                };
            }
            if first == 'f' && rest.bytes().all(|b| b.is_ascii_digit()) && !rest.starts_with('0') {
                let n: u8 = rest.parse().ok()?;
                if (1..=24).contains(&n) {
                    return Some(Key::F(n));
                }
            }
            return None;
        }
    })
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Key::PrintScreen => f.write_str("PrtScr"),
            Key::Pause => f.write_str("Pause"),
            Key::ScrollLock => f.write_str("ScrollLock"),
            Key::Insert => f.write_str("Insert"),
            Key::Delete => f.write_str("Delete"),
            Key::Home => f.write_str("Home"),
            Key::End => f.write_str("End"),
            Key::PageUp => f.write_str("PageUp"),
            Key::PageDown => f.write_str("PageDown"),
            Key::F(n) => write!(f, "F{n}"),
            Key::Letter(c) => write!(f, "{c}"),
            Key::Digit(d) => write!(f, "{d}"),
        }
    }
}

impl fmt::Display for Hotkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let m = self.mods;
        for (on, name) in [(m.ctrl, "Ctrl"), (m.alt, "Alt"), (m.shift, "Shift"), (m.meta, META_NAME)] {
            if on {
                write!(f, "{name}+")?;
            }
        }
        write!(f, "{}", self.key)
    }
}
