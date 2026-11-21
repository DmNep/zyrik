//! «Сохранить как…» (дискета) — системный `IFileSaveDialog`, плюс папка «Изображения»
//! и местное время для имени файла.
//!
//! Флаги, чтобы диалог сам ничего не оставлял: `FOS_DONTADDTORECENT` (не в «Недавние»),
//! `FOS_NOTESTFILECREATE` (не создавать пробный файл для проверки прав).
//! Свою «последнюю папку» Зырик держит в памяти и передаёт через `SetFolder`.

use std::path::{Path, PathBuf};

use windows::core::{w, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{ERROR_CANCELLED, HWND};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FileSaveDialog, FOLDERID_Pictures, IFileSaveDialog, IShellItem, SHCreateItemFromParsingName,
    SHGetKnownFolderPath, FOS_DONTADDTORECENT, FOS_FORCEFILESYSTEM, FOS_NOTESTFILECREATE,
    FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST, KF_FLAG_DEFAULT, SIGDN_FILESYSPATH,
};

use zyrik::export::Stamp;
use zyrik::platform::Problem;

fn os(e: windows::core::Error) -> Problem {
    Problem::Os(e.message())
}

unsafe fn take(p: PWSTR) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let s = p.to_string().ok();
    CoTaskMemFree(Some(p.0 as *const _));
    s
}

pub fn ask(owner: HWND, start: Option<&Path>, name: &str) -> Result<Option<PathBuf>, Problem> {
    unsafe {
        let dlg: IFileSaveDialog = CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER).map_err(os)?;
        let opts = dlg.GetOptions().map_err(os)?;
        dlg.SetOptions(
            opts | FOS_OVERWRITEPROMPT
                | FOS_FORCEFILESYSTEM
                | FOS_PATHMUSTEXIST
                | FOS_DONTADDTORECENT
                | FOS_NOTESTFILECREATE,
        )
        .map_err(os)?;
        let types = [COMDLG_FILTERSPEC { pszName: w!("PNG"), pszSpec: w!("*.png") }];
        dlg.SetFileTypes(&types).map_err(os)?;
        dlg.SetDefaultExtension(w!("png")).map_err(os)?;
        let name = HSTRING::from(name);
        dlg.SetFileName(PCWSTR(name.as_ptr())).map_err(os)?;
        if let Some(dir) = start.filter(|d| d.is_dir()) {
            let dir = HSTRING::from(dir.as_os_str());
            if let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(PCWSTR(dir.as_ptr()), None) {
                let _ = dlg.SetFolder(&item);
            }
        }
        match dlg.Show(Some(owner)) {
            Ok(()) => {}
            Err(e) if e.code() == ERROR_CANCELLED.to_hresult() => return Ok(None),
            Err(e) => return Err(os(e)),
        }
        let item = dlg.GetResult().map_err(os)?;
        let path = take(item.GetDisplayName(SIGDN_FILESYSPATH).map_err(os)?);
        Ok(path.map(PathBuf::from))
    }
}

pub fn pictures() -> Option<PathBuf> {
    unsafe {
        let p = SHGetKnownFolderPath(&FOLDERID_Pictures, KF_FLAG_DEFAULT, None).ok()?;
        take(p).map(PathBuf::from)
    }
}

pub fn now() -> Stamp {
    let t = unsafe { GetLocalTime() };
    Stamp {
        year: t.wYear,
        month: t.wMonth as u8,
        day: t.wDay as u8,
        hour: t.wHour as u8,
        minute: t.wMinute as u8,
        second: t.wSecond as u8,
    }
}
