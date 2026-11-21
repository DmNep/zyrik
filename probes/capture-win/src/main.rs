//! R6: сколько стоит BitBlt всего виртуального экрана (все мониторы, DPI-aware).
//!
//! Запуск: capture-win [N]  — N замеров (по умолчанию 30), печатает min/медиану/max.
//! Пиксели только в памяти; ничего не пишет на диск и в буфер обмена.

use std::time::Instant;
use windows::core::BOOL;
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

unsafe extern "system" fn monitor(h: HMONITOR, _: HDC, _: *mut RECT, _: LPARAM) -> BOOL {
    let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    let _ = GetMonitorInfoW(h, &mut mi);
    let (mut dx, mut dy) = (0u32, 0u32);
    let _ = GetDpiForMonitor(h, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
    let r = mi.rcMonitor;
    println!("  монитор {}x{} в ({}, {}), масштаб {}%", r.right - r.left, r.bottom - r.top, r.left, r.top, dx * 100 / 96);
    true.into()
}

fn main() {
    let n: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(30);
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = EnumDisplayMonitors(None, None, Some(monitor), LPARAM(0));

        let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let h = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        println!("виртуальный экран {w}x{h} в ({x}, {y}), {:.1} МБ RGBA", (w * h * 4) as f64 / 1048576.0);

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
        let bmp = CreateDIBSection(Some(mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0).expect("CreateDIBSection");
        let old = SelectObject(mem, bmp.into());
        let len = (w * h * 4) as usize;

        let mut blt = Vec::with_capacity(n);
        let mut total = Vec::with_capacity(n);
        let mut frame = vec![0u8; len];
        for _ in 0..n {
            let t = Instant::now();
            BitBlt(mem, 0, 0, w, h, Some(screen), x, y, SRCCOPY | CAPTUREBLT).expect("BitBlt");
            let _ = GdiFlush();
            let t1 = t.elapsed();
            std::ptr::copy_nonoverlapping(bits as *const u8, frame.as_mut_ptr(), len);
            blt.push(t1.as_secs_f64() * 1000.0);
            total.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        let nonzero = frame.chunks_exact(4).filter(|p| p[0] | p[1] | p[2] != 0).count();
        println!("непустых пикселей в последнем кадре: {:.1}%", nonzero as f64 * 100.0 / (w * h) as f64);

        // затираем кадр, как будет делать Зырик
        frame.iter_mut().for_each(|b| *b = 0);
        std::ptr::write_bytes(bits as *mut u8, 0, len);

        SelectObject(mem, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);

        for (name, v) in [("BitBlt", &mut blt), ("BitBlt + копия в Vec", &mut total)] {
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            println!("{name:<22} min {:6.1} мс · медиана {:6.1} мс · max {:6.1} мс  (n={n}, первый замер в max)",
                v[0], v[v.len() / 2], v[v.len() - 1]);
        }
    }
}
