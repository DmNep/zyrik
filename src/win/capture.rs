//! Снимок всех мониторов: `BitBlt` виртуального экрана в DIB-секцию (R6: ~33 мс на 1920×1080).
//! Процесс Per-Monitor DPI v2, поэтому координаты и пиксели физические на всех мониторах.

use windows::core::BOOL;
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

use zyrik::image::Image;
use zyrik::platform::{Desktop, Frame, Monitor, Problem};
use zyrik::session::{Pt, Rect};

pub fn capture() -> Result<Frame, Problem> {
    unsafe {
        let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let h = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        if w <= 0 || h <= 0 {
            return Err(Problem::Os("пустой виртуальный экран".into()));
        }
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // сверху вниз
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let result = match CreateDIBSection(Some(mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(bmp) if !bits.is_null() => {
                let old = SelectObject(mem, bmp.into());
                let blt = BitBlt(mem, 0, 0, w, h, Some(screen), x, y, SRCCOPY | CAPTUREBLT);
                let _ = GdiFlush();
                let out = blt.map_err(|e| Problem::Os(e.message())).map(|()| {
                    let n = (w * h) as usize;
                    let src = std::slice::from_raw_parts_mut(bits as *mut u32, n);
                    // альфа после BitBlt не определена — делаем непрозрачной
                    let px: Vec<u32> = src.iter().map(|p| p | 0xFF00_0000).collect();
                    // DIB-секция — тоже копия экрана: затираем до освобождения
                    src.fill(0);
                    Image::from_pixels(w as u32, h as u32, px)
                });
                SelectObject(mem, old);
                let _ = DeleteObject(bmp.into());
                out
            }
            Ok(bmp) => {
                let _ = DeleteObject(bmp.into());
                Err(Problem::Os("CreateDIBSection без памяти".into()))
            }
            Err(e) => Err(Problem::Os(e.message())),
        };
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        let image = result?;
        Ok(Frame { x, y, image, monitors: monitors(x, y), desktop: desktop(x, y) })
    }
}

/// Мониторы в координатах кадра (относительно левого верхнего угла виртуального экрана).
fn monitors(ox: i32, oy: i32) -> Vec<Monitor> {
    struct Ctx {
        ox: i32,
        oy: i32,
        list: Vec<Monitor>,
    }
    unsafe extern "system" fn each(h: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        let ctx = &mut *(data.0 as *mut Ctx);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if GetMonitorInfoW(h, &mut mi).as_bool() {
            let (mut dx, mut dy) = (96u32, 96u32);
            let _ = GetDpiForMonitor(h, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
            let r = mi.rcMonitor;
            ctx.list.push(Monitor {
                rect: Rect { x: r.left - ctx.ox, y: r.top - ctx.oy, w: r.right - r.left, h: r.bottom - r.top },
                scale: dx * 100 / 96,
            });
        }
        true.into()
    }
    let mut ctx = Ctx { ox, oy, list: Vec::new() };
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(each), LPARAM(&mut ctx as *mut Ctx as isize));
    }
    ctx.list
}

/// Окна сверху вниз, активное окно и курсор — в координатах кадра (история 5).
fn desktop(ox: i32, oy: i32) -> Desktop {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS};
    use windows::Win32::UI::WindowsAndMessaging::*;

    /// Видимая рамка окна (без невидимых полей изменения размера); `None` — окно не показываем.
    unsafe fn bounds(h: HWND, ox: i32, oy: i32) -> Option<Rect> {
        if !IsWindowVisible(h).as_bool() || IsIconic(h).as_bool() {
            return None;
        }
        let ex = GetWindowLongW(h, GWL_EXSTYLE) as u32;
        if ex & WS_EX_TOOLWINDOW.0 != 0 && ex & WS_EX_APPWINDOW.0 == 0 {
            return None;
        }
        // окна других виртуальных столов и «спящие» UWP: видимы для API, но не на экране
        let mut cloaked = 0u32;
        let _ = DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut _, 4);
        if cloaked != 0 {
            return None;
        }
        // рабочий стол не окно: щелчок по нему начинает обычное выделение
        let mut class = [0u16; 32];
        let n = GetClassNameW(h, &mut class) as usize;
        let class = String::from_utf16_lossy(&class[..n]);
        if class == "Progman" || class == "WorkerW" {
            return None;
        }
        let mut r = RECT::default();
        if DwmGetWindowAttribute(h, DWMWA_EXTENDED_FRAME_BOUNDS, &mut r as *mut RECT as *mut _, std::mem::size_of::<RECT>() as u32)
            .is_err()
        {
            GetWindowRect(h, &mut r).ok()?;
        }
        (r.right > r.left && r.bottom > r.top)
            .then(|| Rect { x: r.left - ox, y: r.top - oy, w: r.right - r.left, h: r.bottom - r.top })
    }
    struct Ctx {
        ox: i32,
        oy: i32,
        list: Vec<Rect>,
    }
    unsafe extern "system" fn each(h: HWND, data: LPARAM) -> BOOL {
        let ctx = &mut *(data.0 as *mut Ctx);
        if let Some(r) = bounds(h, ctx.ox, ctx.oy) {
            ctx.list.push(r);
        }
        true.into()
    }
    let mut ctx = Ctx { ox, oy, list: Vec::new() };
    unsafe {
        // EnumWindows идёт по окнам верхнего уровня сверху вниз
        let _ = EnumWindows(Some(each), LPARAM(&mut ctx as *mut Ctx as isize));
        let fg = GetForegroundWindow();
        let active = if fg.is_invalid() { None } else { bounds(fg, ox, oy) };
        let mut p = windows::Win32::Foundation::POINT::default();
        let cursor = GetCursorPos(&mut p).ok().map(|()| Pt { x: p.x - ox, y: p.y - oy });
        Desktop { windows: ctx.list, active, cursor }
    }
}
