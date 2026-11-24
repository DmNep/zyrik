//! Снимок в буфер обмена так, чтобы Windows не положила его в журнал Win+V и не отправила
//! в облачный буфер (FR-004), и одноразово: вставили и отпустили Ctrl/Win — буфер пуст (FR-004a).
//! Содержимое буфера программа не читает никогда.
//!
//! Форматы-пометки описаны в документации «Cloud Clipboard and Clipboard History Formats»:
//! - `ExcludeClipboardContentFromMonitorProcessing` — любые данные, сам факт формата;
//! - `CanIncludeInClipboardHistory` = DWORD 0;
//! - `CanUploadToCloudClipboard` = DWORD 0.
//!
//! Картинка кладётся отложенно: `SetClipboardData(CF_DIB, NULL)`. Когда программа её
//! забирает, Windows шлёт нам WM_RENDERFORMAT — так мы узнаём о вставке.

use std::cell::RefCell;

use windows::core::w;
use windows::Win32::Foundation::{
    GetLastError, GlobalFree, SetLastError, HANDLE, HGLOBAL, HWND, LRESULT, WIN32_ERROR, WPARAM,
};
use windows::Win32::Graphics::Gdi::{BITMAPINFOHEADER, BI_RGB};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardOwner, GetOpenClipboardWindow, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::CF_DIB;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_RWIN};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, KillTimer, SetTimer, WM_DESTROYCLIPBOARD,
    WM_RENDERALLFORMATS, WM_RENDERFORMAT, WM_TIMER,
};

use zyrik::image::Image;
use zyrik::oneshot::{Answer, OneShot};
use zyrik::platform::Problem;

/// Таймеры буфера; обычные таймеры `schedule` нумеруются с 1 и сюда не дорастут.
const WATCH_TIMER: usize = usize::MAX - 1;
const REARM_TIMER: usize = usize::MAX - 2;
const WATCH_MS: u32 = 30;
const REARM_MS: u32 = 200;

const CF_DIB_U32: u32 = CF_DIB.0 as u32;

struct State {
    /// Снимок, который мы пообещали буферу. Затирается при удалении.
    image: Option<Image>,
    oneshot: OneShot,
    /// Мы сами вызываем EmptyClipboard — свой же WM_DESTROYCLIPBOARD не считать чужой записью
    emptying: bool,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State { image: None, oneshot: OneShot::new(), emptying: false });
}

macro_rules! trace {
    ($($a:tt)*) => {
        if cfg!(debug_assertions) {
            eprintln!("clip: {}", format!($($a)*));
        }
    };
}

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

/// Обещать формат (отложенная выдача). SetClipboardData с NULL по документации и
/// возвращает NULL — крейт `windows` видит в этом ошибку «Операция успешно завершена»,
/// поэтому смотрим на настоящий код ошибки.
unsafe fn promise_format(format: u32) -> Result<(), Problem> {
    SetLastError(WIN32_ERROR(0));
    match SetClipboardData(format, None) {
        Ok(_) => Ok(()),
        Err(_) if GetLastError() == WIN32_ERROR(0) => Ok(()),
        Err(e) => Err(os(e)),
    }
}

/// DIB снизу вверх: перевёрнутый (top-down) DIB из буфера понимают не все программы.
unsafe fn dib(img: &Image) -> Result<HGLOBAL, Problem> {
    let (w, h) = (img.width() as i32, img.height() as i32);
    let header = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: w,
        biHeight: h,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        biSizeImage: (w * h * 4) as u32,
        ..Default::default()
    };
    let header_bytes = std::slice::from_raw_parts(
        &header as *const BITMAPINFOHEADER as *const u8,
        std::mem::size_of::<BITMAPINFOHEADER>(),
    );
    let hl = header_bytes.len();
    let row = w as usize * 4;
    global(hl + row * h as usize, |dst| {
        dst[..hl].copy_from_slice(header_bytes);
        for y in 0..h as u32 {
            let src = img.row(y);
            let src = std::slice::from_raw_parts(src.as_ptr() as *const u8, row);
            let at = hl + (h as usize - 1 - y as usize) * row;
            dst[at..at + row].copy_from_slice(src);
        }
    })
}

unsafe fn open(owner: HWND) -> Result<(), Problem> {
    // буфер может быть занят другой программой на миг — пробуем несколько раз
    let mut opened = OpenClipboard(Some(owner));
    for _ in 0..10 {
        if opened.is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        opened = OpenClipboard(Some(owner));
    }
    opened.map_err(os)
}

/// EmptyClipboard без того, чтобы считать свой же WM_DESTROYCLIPBOARD чужой записью.
unsafe fn empty_own() -> Result<(), Problem> {
    STATE.with(|s| s.borrow_mut().emptying = true);
    let r = EmptyClipboard().map_err(os);
    STATE.with(|s| s.borrow_mut().emptying = false);
    r
}

/// Пометки «не в журнал, не в облако» и обещание картинки. Буфер должен быть открыт.
unsafe fn promise() -> Result<(), Problem> {
    let exclude = RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing"));
    let history = RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory"));
    let cloud = RegisterClipboardFormatW(w!("CanUploadToCloudClipboard"));
    if exclude == 0 || history == 0 || cloud == 0 {
        return Err(Problem::Os("RegisterClipboardFormat".into()));
    }
    empty_own()?;
    // пометки — до картинки: менеджеры буфера смотрят на них при появлении данных
    put(exclude, global(4, dword_zero)?)?;
    put(history, global(4, dword_zero)?)?;
    put(cloud, global(4, dword_zero)?)?;
    promise_format(CF_DIB_U32)
}

pub fn set_image(owner: HWND, img: Image) -> Result<(), Problem> {
    if img.width() == 0 || img.height() == 0 {
        return Err(Problem::Os("пустой снимок".into()));
    }
    unsafe {
        let _ = KillTimer(Some(owner), WATCH_TIMER);
        let _ = KillTimer(Some(owner), REARM_TIMER);
        open(owner)?;
        let r = promise();
        let _ = CloseClipboard();
        r?;
    }
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.image = Some(img); // прежний снимок, если был, затирается здесь
        s.oneshot = OneShot::new();
    });
    Ok(())
}

/// Просит ли снимок программа на переднем плане — та, в которую вставляют.
/// Сравниваем процесс окна, открывшего буфер, с процессом активного окна.
unsafe fn reader_in_front() -> bool {
    let pid = |w: HWND| {
        let mut pid = 0u32;
        GetWindowThreadProcessId(w, Some(&mut pid));
        pid
    };
    let reader = GetOpenClipboardWindow().map(pid).unwrap_or(0);
    reader != 0 && reader == pid(GetForegroundWindow())
}

fn modifier_held() -> bool {
    unsafe { [VK_CONTROL, VK_LWIN, VK_RWIN].iter().any(|k| GetAsyncKeyState(k.0 as i32) < 0) }
}

fn forget() {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.image = None;
        s.oneshot = OneShot::new();
    });
}

/// Очистить буфер, если там всё ещё наш снимок. `false` — буфер занят, попробовать позже.
unsafe fn clear(owner: HWND) -> bool {
    if OpenClipboard(Some(owner)).is_err() {
        return false;
    }
    if GetClipboardOwner().ok() == Some(owner) {
        let _ = empty_own();
    }
    let _ = CloseClipboard();
    forget();
    true
}

/// `--clear-clipboard`: время вышло. Чужое содержимое не трогаем.
pub fn clear_if_ours(owner: HWND) {
    unsafe {
        let _ = KillTimer(Some(owner), WATCH_TIMER);
        let _ = KillTimer(Some(owner), REARM_TIMER);
        if !clear(owner) {
            // буфер занят на миг — ещё одна попытка
            std::thread::sleep(std::time::Duration::from_millis(100));
            let _ = clear(owner);
        }
    }
}

/// Сообщения буфера и его таймеры для главного окна. `None` — сообщение не наше.
pub fn handle(owner: HWND, msg: u32, wparam: WPARAM) -> Option<LRESULT> {
    unsafe {
        match msg {
            WM_RENDERFORMAT if wparam.0 as u32 == CF_DIB_U32 => {
                let in_front = reader_in_front();
                let held = modifier_held();
                let answer = STATE.with(|s| s.borrow_mut().oneshot.requested(in_front, held));
                trace!("просят снимок: на переднем плане {in_front}, Ctrl/Win {held} → {answer:?}");
                if answer == Answer::Refuse {
                    // Ничего не кладём: читающий получит пусто, обещание останется в буфере.
                    return Some(LRESULT(0));
                }
                // Буфер уже открыт читающей программой: только SetClipboardData, без OpenClipboard.
                let h = STATE.with(|s| s.borrow().image.as_ref().map(|img| dib(img)));
                if let Some(Ok(h)) = h {
                    let _ = put(CF_DIB_U32, h);
                }
                match answer {
                    Answer::GiveAndWatch => {
                        SetTimer(Some(owner), WATCH_TIMER, WATCH_MS, None);
                    }
                    // перевыдаём, когда читающая программа закроет буфер
                    Answer::GiveAndRearm => {
                        SetTimer(Some(owner), REARM_TIMER, REARM_MS, None);
                    }
                    Answer::Give | Answer::Refuse => {}
                }
                Some(LRESULT(0))
            }
            WM_RENDERALLFORMATS => {
                // Мы завершаемся с невыданным обещанием. Не отдаём снимок «на память» — очищаем.
                if OpenClipboard(Some(owner)).is_ok() {
                    if GetClipboardOwner().ok() == Some(owner) {
                        let _ = empty_own();
                    }
                    let _ = CloseClipboard();
                }
                forget();
                Some(LRESULT(0))
            }
            WM_DESTROYCLIPBOARD => {
                // Кто-то другой положил в буфер своё — наш снимок больше не нужен.
                if !STATE.with(|s| s.borrow().emptying) {
                    let _ = KillTimer(Some(owner), WATCH_TIMER);
                    let _ = KillTimer(Some(owner), REARM_TIMER);
                    forget();
                }
                Some(LRESULT(0))
            }
            WM_TIMER if wparam.0 == WATCH_TIMER => {
                let ready = STATE.with(|s| s.borrow().oneshot.should_clear(modifier_held()));
                if ready && clear(owner) {
                    let _ = KillTimer(Some(owner), WATCH_TIMER);
                }
                Some(LRESULT(0))
            }
            WM_TIMER if wparam.0 == REARM_TIMER => {
                let ours = GetClipboardOwner().ok() == Some(owner)
                    && STATE.with(|s| s.borrow().image.is_some());
                if !ours {
                    let _ = KillTimer(Some(owner), REARM_TIMER);
                } else if OpenClipboard(Some(owner)).is_ok() {
                    let _ = promise();
                    let _ = CloseClipboard();
                    let _ = KillTimer(Some(owner), REARM_TIMER);
                }
                // буфер ещё занят читающей программой — повторим на следующем тике
                Some(LRESULT(0))
            }
            _ => None,
        }
    }
}
