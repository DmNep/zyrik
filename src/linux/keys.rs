//! Клавиатура X11: коды клавиш ↔ keysym, буквы для горячих клавиш и символы для подписей.

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt, KeyButMask, ModMask};
use x11rb::rust_connection::RustConnection;

use zyrik::hotkey::Key as HotKey;
use zyrik::session::Key;

/// Таблица «код клавиши → keysym по столбцам» (столбцы: группа 1 без/с Shift, группа 2 …).
pub struct Keymap {
    min: u8,
    per: usize,
    syms: Vec<u32>,
}

impl Keymap {
    pub fn load(conn: &RustConnection) -> Result<Keymap, String> {
        let setup = conn.setup();
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let r = conn
            .get_keyboard_mapping(min, max - min + 1)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        Ok(Keymap { min, per: r.keysyms_per_keycode as usize, syms: r.keysyms })
    }

    fn row(&self, code: u8) -> &[u32] {
        if code < self.min || self.per == 0 {
            return &[];
        }
        let i = (code - self.min) as usize * self.per;
        self.syms.get(i..i + self.per).unwrap_or(&[])
    }

    /// keysym столбца `col`; пустые столбцы второй группы повторяют первую, как в Xlib.
    pub fn sym(&self, code: u8, col: usize) -> u32 {
        let row = self.row(code);
        row.get(col).copied().filter(|&s| s != 0).unwrap_or_else(|| row.get(col % 2).copied().unwrap_or(0))
    }

    /// Все коды клавиш, на которых есть `sym` (в любой раскладке).
    pub fn codes(&self, sym: u32) -> Vec<u8> {
        let n = self.syms.len() / self.per.max(1);
        (0..n)
            .map(|i| self.min + i as u8)
            .filter(|&c| self.row(c).contains(&sym))
            .collect()
    }

    /// Клавиша режима выделения. Буквы — по первой раскладке (обычно латинской), как виртуальные
    /// коды в Windows: P/L/A/R/T работают и при русской раскладке.
    pub fn key(&self, code: u8) -> Option<Key> {
        key_from_sym(self.sym(code, 0))
    }

    /// Набранный символ с учётом раскладки (группа из `state`) и Shift/CapsLock.
    pub fn char(&self, code: u8, state: KeyButMask) -> Option<char> {
        let bits = u16::from(state);
        let group = ((bits >> 13) & 3) as usize;
        let shift = bits & u16::from(KeyButMask::SHIFT) != 0;
        let caps = bits & u16::from(KeyButMask::LOCK) != 0;
        let base = self.sym(code, group * 2);
        let c = keysym_char(self.sym(code, group * 2 + usize::from(shift)))?;
        // CapsLock меняет регистр только у букв
        let c = if caps && keysym_char(base).is_some_and(char::is_alphabetic) {
            if shift { c.to_lowercase().next()? } else { c.to_uppercase().next()? }
        } else {
            c
        };
        (!c.is_control()).then_some(c)
    }

    pub fn is_shift(&self, code: u8) -> bool {
        matches!(self.sym(code, 0), 0xffe1 | 0xffe2)
    }

    /// Ctrl или Super нажат (по карте нажатых клавиш из QueryKeymap) — для одноразового буфера.
    pub fn ctrl_or_super_down(&self, keys: &[u8; 32]) -> bool {
        [0xffe3, 0xffe4, 0xffeb, 0xffec].iter().any(|&sym| {
            self.codes(sym).iter().any(|&c| keys[(c / 8) as usize] & (1 << (c % 8)) != 0)
        })
    }
}

/// Клавиша режима выделения по keysym первой раскладки (буквы — латинские при любой раскладке).
pub fn key_from_sym(sym: u32) -> Option<Key> {
    Some(match sym {
        0xff1b => Key::Esc,
        0xff0d | 0xff8d => Key::Enter,
        0xff08 => Key::Backspace,
        0xff51 | 0xff96 => Key::Left,
        0xff52 | 0xff97 => Key::Up,
        0xff53 | 0xff98 => Key::Right,
        0xff54 | 0xff99 => Key::Down,
        0x61..=0x7a => Key::Letter((sym as u8 - 0x20) as char),
        0x41..=0x5a => Key::Letter(sym as u8 as char),
        _ => return None,
    })
}

/// keysym клавиши горячей комбинации.
pub fn hotkey_sym(k: HotKey) -> u32 {
    match k {
        HotKey::PrintScreen => 0xff61,
        HotKey::Pause => 0xff13,
        HotKey::ScrollLock => 0xff14,
        HotKey::Insert => 0xff63,
        HotKey::Delete => 0xffff,
        HotKey::Home => 0xff50,
        HotKey::End => 0xff57,
        HotKey::PageUp => 0xff55,
        HotKey::PageDown => 0xff56,
        HotKey::F(n) => 0xffbe + u32::from(n) - 1,
        HotKey::Letter(c) => c.to_ascii_lowercase() as u32,
        HotKey::Digit(d) => 0x30 + u32::from(d),
    }
}

pub fn hotkey_mods(m: zyrik::hotkey::Mods) -> ModMask {
    let mut out = ModMask::from(0u16);
    for (on, bit) in [(m.shift, ModMask::SHIFT), (m.ctrl, ModMask::CONTROL), (m.alt, ModMask::M1), (m.meta, ModMask::M4)] {
        if on {
            out |= bit;
        }
    }
    out
}

/// Модификаторы, которые не должны мешать горячей клавише: CapsLock и NumLock.
pub const IGNORED: [u16; 4] = [0, 2, 16, 18];

/// Значимые модификаторы нажатия: Shift, Ctrl, Alt, Super.
pub fn significant(state: KeyButMask) -> u16 {
    u16::from(state) & (1 | 4 | 8 | 64)
}

/// keysym → символ: Latin-1, Unicode-keysym и кириллица.
pub fn keysym_char(sym: u32) -> Option<char> {
    match sym {
        0x20..=0x7e | 0xa0..=0xff => char::from_u32(sym),
        0x0100_0000..=0x0110_ffff => char::from_u32(sym - 0x0100_0000),
        0x6a1..=0x6ff => cyrillic(sym),
        0xffaa..=0xffb9 => keysym_char(sym - 0xff80), // цифровой блок: * + , - . / 0-9
        _ => None,
    }
}

fn cyrillic(sym: u32) -> Option<char> {
    const LOWER: &str = "юабцдефгхийклмнопярстужвьызшэщчъ";
    const UPPER: &str = "ЮАБЦДЕФГХИЙКЛМНОПЯРСТУЖВЬЫЗШЭЩЧЪ";
    match sym {
        0x6c0..=0x6df => LOWER.chars().nth((sym - 0x6c0) as usize),
        0x6e0..=0x6ff => UPPER.chars().nth((sym - 0x6e0) as usize),
        0x6a3 => Some('ё'),
        0x6b3 => Some('Ё'),
        0x6a4 => Some('є'),
        0x6b4 => Some('Є'),
        0x6a6 => Some('і'),
        0x6b6 => Some('І'),
        0x6a7 => Some('ї'),
        0x6b7 => Some('Ї'),
        0x6ad => Some('ґ'),
        0x6bd => Some('Ґ'),
        0x6ae => Some('ў'),
        0x6be => Some('Ў'),
        0x6b0 => Some('№'),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keysyms_to_chars() {
        assert_eq!(keysym_char(0x61), Some('a'));
        assert_eq!(keysym_char(0x6c1), Some('а'));
        assert_eq!(keysym_char(0x6ff), Some('Ъ'));
        assert_eq!(keysym_char(0x6c0), Some('ю'));
        assert_eq!(keysym_char(0x6a3), Some('ё'));
        assert_eq!(keysym_char(0x0100_2192), Some('→'));
        assert_eq!(keysym_char(0xffb5), Some('5'));
        assert_eq!(keysym_char(0xff0d), None);
    }

    fn map() -> Keymap {
        // код 10: a/A | ф/Ф; код 11: Escape; код 12: Shift_L
        Keymap { min: 10, per: 4, syms: vec![0x61, 0x41, 0x6c6, 0x6e6, 0xff1b, 0, 0, 0, 0xffe1, 0, 0, 0] }
    }

    #[test]
    fn letters_follow_first_layout_chars_follow_current() {
        let m = map();
        assert_eq!(m.key(10), Some(Key::Letter('A')));
        assert_eq!(m.key(11), Some(Key::Esc));
        assert!(m.is_shift(12));
        assert_eq!(m.char(10, KeyButMask::from(0u16)), Some('a'));
        assert_eq!(m.char(10, KeyButMask::SHIFT), Some('A'));
        let ru = KeyButMask::from(1u16 << 13);
        assert_eq!(m.char(10, ru), Some('ф'));
        assert_eq!(m.char(10, ru | KeyButMask::LOCK), Some('Ф'));
        assert_eq!(m.char(11, ru), None, "Esc — не символ");
        assert_eq!(m.codes(0x6c6), vec![10]);
    }
}
