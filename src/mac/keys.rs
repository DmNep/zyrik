//! Коды клавиш macOS (kVK_*, не зависят от раскладки): горячие клавиши и клавиши режима выделения.

use zyrik::hotkey::Key as HotKey;
use zyrik::session::Key;

/// Буквы и цифры ANSI-клавиатуры: (kVK, символ).
const ANSI: [(u16, char); 36] = [
    (0x00, 'A'), (0x0B, 'B'), (0x08, 'C'), (0x02, 'D'), (0x0E, 'E'), (0x03, 'F'), (0x05, 'G'),
    (0x04, 'H'), (0x22, 'I'), (0x26, 'J'), (0x28, 'K'), (0x25, 'L'), (0x2E, 'M'), (0x2D, 'N'),
    (0x1F, 'O'), (0x23, 'P'), (0x0C, 'Q'), (0x0F, 'R'), (0x01, 'S'), (0x11, 'T'), (0x20, 'U'),
    (0x09, 'V'), (0x0D, 'W'), (0x07, 'X'), (0x10, 'Y'), (0x06, 'Z'),
    (0x1D, '0'), (0x12, '1'), (0x13, '2'), (0x14, '3'), (0x15, '4'), (0x17, '5'), (0x16, '6'),
    (0x1A, '7'), (0x1C, '8'), (0x19, '9'),
];

/// F1…F20
const F: [u16; 20] = [
    122, 120, 99, 118, 96, 97, 98, 100, 101, 109, 103, 111, 105, 107, 113, 106, 64, 79, 80, 90,
];

/// Код клавиши для горячей комбинации. На клавиатурах Mac нет PrtScr/ScrollLock/Pause —
/// их место занимают F13/F14/F15, как принято в macOS.
pub fn hotkey_code(k: HotKey) -> Option<u16> {
    Some(match k {
        HotKey::PrintScreen => 105,
        HotKey::ScrollLock => 107,
        HotKey::Pause => 113,
        HotKey::Insert => 114, // Help
        HotKey::Delete => 117, // Forward Delete
        HotKey::Home => 115,
        HotKey::End => 119,
        HotKey::PageUp => 116,
        HotKey::PageDown => 121,
        HotKey::F(n) => *F.get(usize::from(n).checked_sub(1)?)?,
        HotKey::Letter(c) => ANSI.iter().find(|(_, ch)| *ch == c)?.0,
        HotKey::Digit(d) => ANSI.iter().find(|(_, ch)| ch.to_digit(10) == Some(u32::from(d)))?.0,
    })
}

/// Клавиша режима выделения по коду (буквы — по позиции, как виртуальные коды в Windows).
pub fn key(code: u16) -> Option<Key> {
    Some(match code {
        53 => Key::Esc,
        36 | 76 => Key::Enter,
        51 => Key::Backspace,
        123 => Key::Left,
        124 => Key::Right,
        125 => Key::Down,
        126 => Key::Up,
        _ => {
            let c = ANSI.iter().find(|(k, _)| *k == code)?.1;
            if !c.is_ascii_alphabetic() {
                return None;
            }
            Key::Letter(c)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes() {
        assert_eq!(hotkey_code(HotKey::Digit(2)), Some(0x13), "Cmd+Shift+2 — клавиша по умолчанию");
        assert_eq!(hotkey_code(HotKey::F(1)), Some(122));
        assert_eq!(hotkey_code(HotKey::F(24)), None);
        assert_eq!(key(0x23), Some(Key::Letter('P')));
        assert_eq!(key(0x13), None, "цифры — не буквы");
        assert_eq!(key(53), Some(Key::Esc));
    }
}
