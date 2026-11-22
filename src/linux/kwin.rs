//! Захват в KDE Plasma: `org.kde.KWin.ScreenShot2.CaptureWorkspace` — KWin пишет пиксели прямо
//! в трубу (pipe), без файла на диске и без диалога. Так снимают Spectacle и Flameshot.
//!
//! KWin пускает к этому интерфейсу только программы с `.desktop`-файлом, где есть
//! `X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2` (см. README, раздел KDE).
//! Без него — ошибка `NoAuthorized`, и тогда Зырик пробует портал.

use std::collections::HashMap;
use std::io::Read;
use std::os::fd::OwnedFd;

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedValue, Value};

use zyrik::image::Image;
use zyrik::platform::Problem;

/// Почему KWin не дал снимок.
pub enum Fail {
    /// Нет `.desktop` с разрешением — можно попробовать портал
    NotAuthorized,
    Other(Problem),
}

fn other<E: std::fmt::Display>(e: E) -> Fail {
    Fail::Other(Problem::Os(format!("KWin: {e}")))
}

fn get_u32(m: &HashMap<String, OwnedValue>, k: &str) -> Option<u32> {
    m.get(k).and_then(|v| u32::try_from(v.clone()).ok())
}

/// Весь рабочий стол в физических пикселях.
pub fn capture() -> Result<Image, Fail> {
    if !super::tray::bus_is_local() {
        return Err(other("адрес шины D-Bus не unix:"));
    }
    let conn = Connection::session().map_err(other)?;
    let proxy = Proxy::new(&conn, "org.kde.KWin.ScreenShot2", "/org/kde/KWin/ScreenShot2", "org.kde.KWin.ScreenShot2").map_err(other)?;
    let mut fds = [0; 2];
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(other(std::io::Error::last_os_error()));
    }
    let (rd, wr): (OwnedFd, OwnedFd) = unsafe { (std::os::fd::FromRawFd::from_raw_fd(fds[0]), std::os::fd::FromRawFd::from_raw_fd(fds[1])) };
    // KWin пишет в трубу, пока мы ждём ответ на вызов, — читаем в отдельном потоке
    let reader = std::thread::spawn(move || {
        let mut f = std::fs::File::from(rd);
        let mut buf = zeroize::Zeroizing::new(Vec::new());
        let _ = f.read_to_end(&mut buf);
        buf
    });
    let mut opts: HashMap<&str, Value> = HashMap::new();
    opts.insert("include-cursor", Value::from(false));
    opts.insert("native-resolution", Value::from(true));
    let reply: Result<HashMap<String, OwnedValue>, zbus::Error> =
        proxy.call("CaptureWorkspace", &(opts, zbus::zvariant::Fd::from(&wr)));
    drop(wr);
    let data = reader.join().map_err(|_| other("поток чтения"))?;
    let meta = match reply {
        Ok(m) => m,
        Err(zbus::Error::MethodError(name, _, _)) if name.as_str().ends_with("NoAuthorized") => return Err(Fail::NotAuthorized),
        Err(e) => return Err(other(e)),
    };
    let (w, h) = (get_u32(&meta, "width").ok_or_else(|| other("нет ширины"))?, get_u32(&meta, "height").ok_or_else(|| other("нет высоты"))?);
    let stride = get_u32(&meta, "stride").unwrap_or(w * 4) as usize;
    // QImage::Format_RGB32 (4), ARGB32 (5), ARGB32_Premultiplied (6): в памяти B, G, R, A
    let format = get_u32(&meta, "format").unwrap_or(4);
    if !(4..=6).contains(&format) {
        return Err(other(format!("формат картинки {format}")));
    }
    if data.len() < stride * h as usize {
        return Err(other("снимок пришёл не целиком"));
    }
    let mut px = Vec::with_capacity((w * h) as usize);
    for y in 0..h as usize {
        for b in data[y * stride..y * stride + w as usize * 4].as_chunks::<4>().0 {
            px.push(0xFF00_0000 | u32::from_le_bytes([b[0], b[1], b[2], 0]));
        }
    }
    Ok(Image::from_pixels(w, h, px))
}
