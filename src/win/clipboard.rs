//! Снимок в буфер обмена так, чтобы Windows не положила его в журнал Win+V и не отправила
//! в облачный буфер (FR-004). Буфер обмена программа не читает никогда.
//!
//! Форматы-пометки описаны в документации «Cloud Clipboard and Clipboard History Formats»:
//! - `ExcludeClipboardContentFromMonitorProcessing` — любые данные, сам факт формата;
//! - `CanIncludeInClipboardHistory` = DWORD 0;
//! - `CanUploadToCloudClipboard` = DWORD 0.

use windows::core::w;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::Graphics::Gdi::{BITMAPINFOHEADER, BI_RGB};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::CF_DIB;

use zyrik::image::Image;
use zyrik::platform::Problem;

fn os(e: windows::core::Error) -> Problem {
    Problem::Os(e.message())
}

/// Глобальная память под буфер обмена: `fill` пишет прямо в неё, без промежуточных копий снимка.
unsafe fn global(len: usize, fill: impl FnOnce(&mut [u8])) -> Result<HGLOBAL, Problem> {
    let h = GlobalAlloc(GMEM_MOVEABLE, len).map_err(os)?;
    let dst = GlobalLock(h) as *mut u8;
    if dst.is_null() {
        let _ = GlobalFree(Some(h));
        return Err(Problem::Os("GlobalLock".into()));
    }
    fill(std::slice::from_raw_parts_mut(dst, len));
    let _ = GlobalUnlock(h);
    Ok(h)
}

fn dword_zero(dst: &mut [u8]) {
    dst.copy_from_slice(&0u32.to_ne_bytes());
}

/// Отдать память буферу; если не взял — освободить самим.
unsafe fn put(format: u32, h: HGLOBAL) -> Result<(), Problem> {
    match SetClipboardData(format, Some(HANDLE(h.0))) {
        Ok(_) => Ok(()),
        Err(e) => {
            let _ = GlobalFree(Some(h));
            Err(os(e))
        }
    }
}

pub fn set_image(owner: HWND, img: &Image) -> Result<(), Problem> {
    let (w, h) = (img.width() as i32, img.height() as i32);
    if w == 0 || h == 0 {
        return Err(Problem::Os("пустой снимок".into()));
    }
    let header = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: w,
        biHeight: h, // снизу вверх: перевёрнутый (top-down) DIB из буфера понимают не все программы
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        biSizeImage: (w * h * 4) as u32,
        ..Default::default()
    };
    unsafe {
        let header_bytes = std::slice::from_raw_parts(
            &header as *const BITMAPINFOHEADER as *const u8,
            std::mem::size_of::<BITMAPINFOHEADER>(),
        );
        let hl = header_bytes.len();
        let row = w as usize * 4;
        let dib_len = hl + row * h as usize;
        let write_dib = |dst: &mut [u8]| {
            dst[..hl].copy_from_slice(header_bytes);
            for y in 0..h as u32 {
                let src = img.row(y);
                let src = std::slice::from_raw_parts(src.as_ptr() as *const u8, row);
                let at = hl + (h as usize - 1 - y as usize) * row;
                dst[at..at + row].copy_from_slice(src);
            }
        };

        // форматы-пометки регистрируются один раз на сеанс Windows; повторный вызов отдаёт тот же номер
        let exclude = RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing"));
        let history = RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory"));
        let cloud = RegisterClipboardFormatW(w!("CanUploadToCloudClipboard"));
        if exclude == 0 || history == 0 || cloud == 0 {
            return Err(Problem::Os("RegisterClipboardFormat".into()));
        }

        // буфер может быть занят другой программой на миг — пробуем несколько раз
        let mut opened = OpenClipboard(Some(owner));
        for _ in 0..10 {
            if opened.is_ok() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
            opened = OpenClipboard(Some(owner));
        }
        opened.map_err(os)?;

        let result = (|| {
            EmptyClipboard().map_err(os)?;
            // пометки — до картинки: менеджеры буфера смотрят на них при появлении данных
            put(exclude, global(4, dword_zero)?)?;
            put(history, global(4, dword_zero)?)?;
            put(cloud, global(4, dword_zero)?)?;
            put(u32::from(CF_DIB.0), global(dib_len, write_dib)?)
        })();
        let _ = CloseClipboard();
        result
    }
}

