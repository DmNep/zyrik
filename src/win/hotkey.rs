//! Глобальные клавиши через `RegisterHotKey` (R1: Ctrl+PrtScr ловится, на SendInput тоже).

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
    MOD_SHIFT, MOD_WIN,
};

use zyrik::hotkey::{Hotkey, Key};
use zyrik::platform::Problem;

/// HRESULT_FROM_WIN32(ERROR_HOTKEY_ALREADY_REGISTERED)
const ALREADY_REGISTERED: i32 = 0x8007_0581_u32 as i32;

pub fn vk(key: Key) -> u32 {
    match key {
        Key::PrintScreen => 0x2C,
        Key::Pause => 0x13,
        Key::ScrollLock => 0x91,
        Key::Insert => 0x2D,
        Key::Delete => 0x2E,
        Key::Home => 0x24,
        Key::End => 0x23,
        Key::PageUp => 0x21,
        Key::PageDown => 0x22,
        Key::F(n) => 0x70 + u32::from(n) - 1,
        Key::Letter(c) => c as u32,
        Key::Digit(d) => 0x30 + u32::from(d),
    }
}

pub fn register(hwnd: HWND, id: i32, key: Hotkey) -> Result<(), Problem> {
    let m = key.mods;
    let mut mods = MOD_NOREPEAT;
    for (on, flag) in [(m.ctrl, MOD_CONTROL), (m.shift, MOD_SHIFT), (m.alt, MOD_ALT), (m.meta, MOD_WIN)] {
        if on {
            mods |= flag;
        }
    }
    unsafe { RegisterHotKey(Some(hwnd), id, HOT_KEY_MODIFIERS(mods.0), vk(key.key)) }.map_err(|e| {
        if e.code().0 == ALREADY_REGISTERED {
            Problem::KeyBusy
        } else {
            Problem::Os(e.message())
        }
    })
}

pub fn unregister(hwnd: HWND, id: i32) {
    unsafe {
        let _ = UnregisterHotKey(Some(hwnd), id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_key_codes() {
        assert_eq!(vk(Key::PrintScreen), 0x2C);
        assert_eq!(vk(Key::F(1)), 0x70);
        assert_eq!(vk(Key::F(24)), 0x87);
        assert_eq!(vk(Key::Letter('W')), 0x57);
        assert_eq!(vk(Key::Digit(2)), 0x32);
    }
}
