//! Окно выделения: топмост без рамки на весь виртуальный экран. Создаётся при запуске
//! и прячется — показ занимает миллисекунды (принцип II). Само ничего не рисует и не решает:
//! мышь и клавиши уходят в ядро, готовую картинку ядро присылает через `present`.

use windows::core::w;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, SetFocus, VIRTUAL_KEY, VK_C, VK_CONTROL, VK_DOWN,
    VK_ESCAPE, VK_LEFT, VK_RETURN, VK_RIGHT, VK_SHIFT, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::*;

use zyrik::image::Image;
use zyrik::platform::{Command, Problem};
use zyrik::session::{Button, Cursor, Input, Key, Pt};

use super::{push, with};

const CLASS: windows::core::PCWSTR = w!("ZyrikOverlay");

pub fn create(instance: HINSTANCE) -> Result<HWND, String> {
    unsafe {
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_DBLCLKS,
            lpfnWndProc: Some(wnd_proc),
            hInstance: instance,
            lpszClassName: CLASS,
            ..Default::default()
        };
        if RegisterClassExW(&class) == 0 {
            return Err("RegisterClassExW overlay".into());
        }
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
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
        .map_err(|e| e.message())
    }
}

pub fn show(hwnd: HWND, x: i32, y: i32, w: u32, h: u32) -> Result<(), Problem> {
    unsafe {
        SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, w as i32, h as i32, SWP_SHOWWINDOW)
            .map_err(|e| Problem::Os(e.message()))?;
        // клавиатура нужна для Esc/Enter/стрелок; после горячей клавиши ОС это разрешает
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(Some(hwnd));
    }
    Ok(())
}

/// Вывести картинку 1:1 в клиентскую область.
pub fn present(hwnd: HWND, img: &Image) {
    let (w, h) = (img.width() as i32, img.height() as i32);
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    unsafe {
        let dc = GetDC(Some(hwnd));
        SetDIBitsToDevice(
            dc,
            0,
            0,
            w as u32,
            h as u32,
            0,
            0,
            0,
            h as u32,
            img.pixels().as_ptr() as *const _,
            &bmi,
            DIB_RGB_COLORS,
        );
        ReleaseDC(Some(hwnd), dc);
        let _ = ValidateRect(Some(hwnd), None);
    }
}

pub fn hide(hwnd: HWND) {
    unsafe {
        let _ = ReleaseCapture();
        // Закрасить чёрным перед тем, как спрятать: иначе в поверхности окна у DWM
        // остался бы последний кадр со снимком и мог мелькнуть при следующем показе.
        let dc = GetDC(Some(hwnd));
        let mut rc = windows::Win32::Foundation::RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        let _ = PatBlt(dc, 0, 0, rc.right, rc.bottom, BLACKNESS);
        ReleaseDC(Some(hwnd), dc);
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
}

fn cursor_id(c: Cursor) -> windows::core::PCWSTR {
    match c {
        Cursor::Cross => IDC_CROSS,
        Cursor::Move => IDC_SIZEALL,
        Cursor::ResizeNwse => IDC_SIZENWSE,
        Cursor::ResizeNesw => IDC_SIZENESW,
        Cursor::ResizeNs => IDC_SIZENS,
        Cursor::ResizeWe => IDC_SIZEWE,
    }
}

pub fn apply_cursor(c: Cursor) {
    unsafe {
        if let Ok(h) = LoadCursorW(None, cursor_id(c)) {
            SetCursor(Some(h));
        }
    }
}

fn point(lparam: LPARAM) -> Pt {
    // GET_X_LPARAM / GET_Y_LPARAM: координаты со знаком
    Pt { x: (lparam.0 & 0xFFFF) as i16 as i32, y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32 }
}

fn key(vk: u16) -> Option<Key> {
    Some(match VIRTUAL_KEY(vk) {
        VK_ESCAPE => Key::Esc,
        VK_RETURN => Key::Enter,
        VK_C => Key::C,
        VK_LEFT => Key::Left,
        VK_RIGHT => Key::Right,
        VK_UP => Key::Up,
        VK_DOWN => Key::Down,
        _ => return None,
    })
}

fn pressed(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetKeyState(vk.0 as i32) < 0 }
}

fn send(i: Input) {
    push(Command::Overlay(i));
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_LBUTTONDOWN => {
            SetCapture(hwnd);
            send(Input::Down(Button::Left, point(lparam)));
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let _ = ReleaseCapture();
            send(Input::Up(Button::Left, point(lparam)));
            LRESULT(0)
        }
        WM_LBUTTONDBLCLK => {
            send(Input::DoubleClick(point(lparam)));
            LRESULT(0)
        }
        WM_RBUTTONDOWN => {
            send(Input::Down(Button::Right, point(lparam)));
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            send(Input::Move(point(lparam)));
            LRESULT(0)
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            if let Some(k) = key(wparam.0 as u16) {
                send(Input::Key { key: k, shift: pressed(VK_SHIFT), ctrl: pressed(VK_CONTROL) });
                LRESULT(0)
            } else {
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
        }
        WM_SETCURSOR if (lparam.0 & 0xFFFF) as u32 == HTCLIENT => {
            apply_cursor(with(|s| s.cursor));
            LRESULT(1)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            BeginPaint(hwnd, &mut ps);
            let _ = EndPaint(hwnd, &ps);
            send(Input::Repaint);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        // окно уже в физических пикселях на весь виртуальный экран — двигать его не даём
        WM_DPICHANGED => LRESULT(0),
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
