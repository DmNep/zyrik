//! Известные папки и диски для выбора папки (`zyrik::picker`) и местное время для имени файла.
//!
//! Системный диалог «Сохранить как…» здесь больше не используется: даже с `FOS_DONTADDTORECENT`
//! он пишет в реестр (`ComDlg32\LastVisitedPidlMRU`) имя программы и последнюю папку.
//! Здесь только узнаём пути: `SHGetKnownFolderPath` и `GetLogicalDrives` ничего не записывают.

use std::path::PathBuf;

use windows::core::GUID;
use windows::Win32::Storage::FileSystem::GetLogicalDrives;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Pictures, SHGetKnownFolderPath,
    KF_FLAG_DEFAULT,
};

use zyrik::export::Stamp;
use zyrik::platform::{Place, PlaceKind};

fn known(id: &GUID) -> Option<PathBuf> {
    unsafe {
        let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s.map(PathBuf::from)
    }
}

/// «Изображения» первой: она же папка по умолчанию для Ctrl+S.
pub fn places() -> Vec<Place> {
    let folders = [
        (PlaceKind::Pictures, &FOLDERID_Pictures),
        (PlaceKind::Desktop, &FOLDERID_Desktop),
        (PlaceKind::Documents, &FOLDERID_Documents),
        (PlaceKind::Downloads, &FOLDERID_Downloads),
    ];
    let mut out: Vec<Place> =
        folders.iter().filter_map(|(kind, id)| known(id).map(|path| Place { kind: *kind, path })).collect();
    let mask = unsafe { GetLogicalDrives() };
    for i in 0..26u32 {
        if mask & (1 << i) != 0 {
            let letter = char::from(b'A' + i as u8);
            out.push(Place { kind: PlaceKind::Drive, path: PathBuf::from(format!("{letter}:\\")) });
        }
    }
    out
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
