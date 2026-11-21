//! Значок «о_О» в трее и его меню (как в Зябре: `Shell_NotifyIcon`).

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, POINT, WPARAM};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, GetSystemMetrics, LoadIconW,
    LoadImageW, PostMessageW, SetForegroundWindow, SetMenuDefaultItem, TrackPopupMenuEx, HICON,
    IDI_APPLICATION, IMAGE_ICON, LR_DEFAULTCOLOR, MF_SEPARATOR, MF_STRING, SM_CXSMICON,
    SM_CYSMICON, TPM_BOTTOMALIGN, TPM_RIGHTALIGN, WM_NULL,
};

use super::utf16;

/// Сообщение, которое шлёт значок трея нашему окну.
pub const WM_TRAY: u32 = 0x8000 + 1;
/// Команды меню: `MENU_BASE + индекс пункта`.
pub const MENU_BASE: usize = 100;

pub fn load_icon(instance: HINSTANCE) -> HICON {
    unsafe {
        // Ресурс 1 вшит build.rs; берём кадр под размер значка трея при текущем DPI.
        // Не LoadIconMetric: она есть только в comctl32 v6.
        let (w, h) = (GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON));
        if let Ok(h) = LoadImageW(Some(instance), PCWSTR(std::ptr::without_provenance(1)) /* MAKEINTRESOURCE(1) */, IMAGE_ICON, w, h, LR_DEFAULTCOLOR) {
            return HICON(h.0);
        }
        LoadIconW(None, IDI_APPLICATION).unwrap_or_default()
    }
}

fn data(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        ..Default::default()
    }
}

fn fill_tip(d: &mut NOTIFYICONDATAW, tip: &str) {
    d.szTip = [0; 128];
    for (i, u) in tip.encode_utf16().take(127).enumerate() {
        d.szTip[i] = u;
    }
}

pub fn add(hwnd: HWND, icon: HICON, tip: &str) -> Result<(), String> {
    let mut d = data(hwnd);
    d.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    d.uCallbackMessage = WM_TRAY;
    d.hIcon = icon;
    fill_tip(&mut d, tip);
    if unsafe { Shell_NotifyIconW(NIM_ADD, &d) }.as_bool() {
        Ok(())
    } else {
        Err("Shell_NotifyIcon(NIM_ADD)".into())
    }
}

pub fn set_tip(hwnd: HWND, tip: &str) {
    let mut d = data(hwnd);
    d.uFlags = NIF_TIP;
    fill_tip(&mut d, tip);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &d);
    }
}

pub fn remove(hwnd: HWND) {
    let d = data(hwnd);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &d);
    }
}

/// Показать меню у курсора. Выбранный пункт придёт как WM_COMMAND `MENU_BASE + i`.
/// Перед «Выходом» — разделитель; первый пункт жирный (то же, что левый щелчок).
pub fn show_menu(hwnd: HWND, items: &[String]) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        for (i, text) in items.iter().enumerate() {
            if i + 1 == items.len() {
                let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            }
            let w = utf16(text);
            let _ = AppendMenuW(menu, MF_STRING, MENU_BASE + i, PCWSTR(w.as_ptr()));
        }
        let _ = SetMenuDefaultItem(menu, MENU_BASE as u32, 0);
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        // без этого меню не закрывается при щелчке мимо
        let _ = SetForegroundWindow(hwnd);
        let _ = TrackPopupMenuEx(menu, (TPM_RIGHTALIGN | TPM_BOTTOMALIGN).0, pt.x, pt.y, hwnd, None);
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
    }
}
