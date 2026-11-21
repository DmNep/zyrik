//! Прослойка Windows. Состояние окна — в thread_local: оконная процедура вызывается ОС
//! без нашего `self`, а всё работает в одном потоке.

mod hotkey;
mod single_instance;
mod tray;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Globalization::{GetUserDefaultUILanguage, LCIDToLocaleName};
use windows::Win32::System::Console::{AttachConsole, FreeConsole, WriteConsoleW, ATTACH_PARENT_PROCESS};
use windows::Win32::System::Diagnostics::Debug::OutputDebugStringW;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::WindowsAndMessaging::*;

use zyrik::hotkey::Hotkey;
use zyrik::platform::{Command, Frame, MenuItem, Platform, Problem, Shot};

const CLASS: PCWSTR = w!("ZyrikWindow");

#[derive(Default)]
struct Shared {
    queue: VecDeque<Command>,
    menu: Vec<MenuItem>,
    /// `hotkeys[id - 1]` — что снимать по WM_HOTKEY с этим id
    hotkeys: Vec<Shot>,
    timers: Vec<(usize, Command)>,
    next_timer: usize,
    tip: String,
    icon: HICON,
    tray_shown: bool,
    taskbar_created: u32,
    ending: bool,
}

thread_local! {
    static SHARED: RefCell<Shared> = RefCell::new(Shared::default());
}

fn with<R>(f: impl FnOnce(&mut Shared) -> R) -> R {
    SHARED.with(|s| f(&mut s.borrow_mut()))
}

pub fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

pub struct Win {
    hwnd: HWND,
    instance: HINSTANCE,
    guard: Option<single_instance::Guard>,
}

impl Win {
    pub fn new() -> Result<Win, String> {
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let instance: HINSTANCE = GetModuleHandleW(None).map_err(|e| e.message())?.into();
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(wnd_proc),
                hInstance: instance,
                lpszClassName: CLASS,
                ..Default::default()
            };
            if RegisterClassExW(&class) == 0 {
                return Err("RegisterClassExW".into());
            }
            // Невидимое окно: принимает WM_HOTKEY, сообщения трея и таймеры.
            // Не message-only: иначе меню трея не закрывается щелчком мимо.
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                CLASS,
                w!("Zyrik"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance),
                None,
            )
            .map_err(|e| e.message())?;
            with(|s| s.taskbar_created = RegisterWindowMessageW(w!("TaskbarCreated")));
            Ok(Win { hwnd, instance, guard: None })
        }
    }
}

impl Drop for Win {
    fn drop(&mut self) {
        let (ids, shown) = with(|s| (s.hotkeys.len(), s.tray_shown));
        for id in 1..=ids {
            hotkey::unregister(self.hwnd, id as i32);
        }
        if shown {
            tray::remove(self.hwnd);
        }
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
        self.guard = None;
    }
}

impl Platform for Win {
    fn system_lang(&self) -> Option<String> {
        unsafe {
            let id = GetUserDefaultUILanguage();
            let mut buf = [0u16; 85];
            let n = LCIDToLocaleName(u32::from(id), Some(&mut buf), 0);
            (n > 1).then(|| String::from_utf16_lossy(&buf[..n as usize - 1]))
        }
    }

    fn claim_single_instance(&mut self) -> Result<(), Problem> {
        self.guard = Some(single_instance::Guard::claim()?);
        Ok(())
    }

    fn show_tray(&mut self, tip: &str, menu: &[MenuItem]) -> Result<(), Problem> {
        let icon = tray::load_icon(self.instance);
        with(|s| {
            s.menu = menu.to_vec();
            s.tip = tip.to_string();
            s.icon = icon;
        });
        tray::add(self.hwnd, icon, tip).map_err(Problem::Os)?;
        with(|s| s.tray_shown = true);
        Ok(())
    }

    fn set_tray_tip(&mut self, tip: &str) {
        with(|s| s.tip = tip.to_string());
        tray::set_tip(self.hwnd, tip);
    }

    fn register_hotkey(&mut self, shot: Shot, key: Hotkey) -> Result<(), Problem> {
        let id = with(|s| {
            s.hotkeys.push(shot);
            s.hotkeys.len() as i32
        });
        hotkey::register(self.hwnd, id, key)
    }

    fn schedule(&mut self, command: Command, delay_ms: u32) {
        let id = with(|s| {
            s.next_timer += 1;
            s.timers.push((s.next_timer, command));
            s.next_timer
        });
        unsafe {
            SetTimer(Some(self.hwnd), id, delay_ms, None);
        }
    }

    fn next_command(&mut self) -> Option<Command> {
        loop {
            if let Some(c) = with(|s| s.queue.pop_front()) {
                return Some(c);
            }
            if with(|s| s.ending) {
                return None;
            }
            let mut msg = MSG::default();
            unsafe {
                if !GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    return None;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    fn capture(&mut self) -> Result<Frame, Problem> {
        Err(Problem::NotYet("capture: фаза 2"))
    }

    fn show_overlay(&mut self, _frame: &Frame) -> Result<(), Problem> {
        Err(Problem::NotYet("overlay: фаза 2"))
    }

    fn set_clipboard_image(&mut self, _w: u32, _h: u32, _rgba: &[u8]) -> Result<(), Problem> {
        Err(Problem::NotYet("clipboard: фаза 2"))
    }

    fn save_dialog(&mut self, _start: &Path, _name: &str) -> Result<Option<PathBuf>, Problem> {
        Err(Problem::NotYet("save dialog: фаза 4"))
    }

    fn message(&self, title: &str, text: &str, error: bool) {
        message_box(title, text, error);
    }

    fn debug_log(&self, text: &str) {
        use std::io::Write;
        let w = utf16(&format!("zyrik: {text}\n"));
        unsafe { OutputDebugStringW(PCWSTR(w.as_ptr())) };
        let _ = writeln!(std::io::stderr(), "zyrik: {text}");
    }
}

fn push(c: Command) {
    with(|s| s.queue.push_back(c));
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_HOTKEY => {
            let shot = with(|s| s.hotkeys.get(wparam.0.wrapping_sub(1)).copied());
            if let Some(shot) = shot {
                push(Command::Capture(shot));
            }
            LRESULT(0)
        }
        tray::WM_TRAY => {
            match (lparam.0 as u32) & 0xFFFF {
                WM_LBUTTONUP => {
                    if let Some(first) = with(|s| s.menu.first().map(|m| m.command.clone())) {
                        push(first);
                    }
                }
                WM_RBUTTONUP | WM_CONTEXTMENU => {
                    // borrow отпущен до TrackPopupMenu: меню крутит свой цикл сообщений
                    let items: Vec<String> = with(|s| s.menu.iter().map(|m| m.text.clone()).collect());
                    tray::show_menu(hwnd, &items);
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let i = (wparam.0 & 0xFFFF).wrapping_sub(tray::MENU_BASE);
            if let Some(c) = with(|s| s.menu.get(i).map(|m| m.command.clone())) {
                push(c);
            }
            LRESULT(0)
        }
        WM_TIMER => {
            let _ = KillTimer(Some(hwnd), wparam.0);
            let c = with(|s| {
                let at = s.timers.iter().position(|(id, _)| *id == wparam.0)?;
                Some(s.timers.remove(at).1)
            });
            if let Some(c) = c {
                push(c);
            }
            LRESULT(0)
        }
        WM_ENDSESSION => {
            if wparam.0 != 0 {
                with(|s| s.ending = true);
            }
            LRESULT(0)
        }
        _ if msg != 0 && msg == with(|s| s.taskbar_created) => {
            // Explorer перезапустился — значок пропал, ставим заново
            let (icon, tip, shown) = with(|s| (s.icon, s.tip.clone(), s.tray_shown));
            if shown {
                let _ = tray::add(hwnd, icon, &tip);
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

pub fn message_box(title: &str, text: &str, error: bool) {
    let t = utf16(text);
    let c = utf16(title);
    let icon = if error { MB_ICONWARNING } else { MB_ICONINFORMATION };
    unsafe {
        let _ = MessageBoxW(None, PCWSTR(t.as_ptr()), PCWSTR(c.as_ptr()), MB_OK | icon | MB_SETFOREGROUND | MB_TOPMOST);
    }
}

/// Справка и ошибки параметров: в консоль, из которой запустили; если её нет — окном.
/// У релизной сборки своей консоли нет (windows_subsystem), поэтому цепляемся к родительской.
pub fn explain(title: &str, text: &str, error: bool) {
    if cfg!(debug_assertions) {
        // отладочная сборка — консольная программа, консоль есть всегда
        use std::io::Write;
        let _ = if error {
            writeln!(std::io::stderr(), "{text}")
        } else {
            writeln!(std::io::stdout(), "{text}")
        };
        return;
    }
    unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS).is_ok() {
            if let Ok(file) = std::fs::OpenOptions::new().write(true).open("CONOUT$") {
                let handle = HANDLE(std::os::windows::io::AsRawHandle::as_raw_handle(&file));
                let mut wide: Vec<u16> = text.encode_utf16().collect();
                wide.extend([u16::from(b'\r'), u16::from(b'\n')]);
                let ok = WriteConsoleW(handle, &wide, None, None).is_ok();
                drop(file);
                let _ = FreeConsole();
                if ok {
                    return;
                }
            } else {
                let _ = FreeConsole();
            }
        }
    }
    message_box(title, text, error);
}
