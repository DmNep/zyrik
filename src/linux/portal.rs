//! Захват через портал `org.freedesktop.portal.Screenshot` — крайний случай для композиторов без
//! `wlr-screencopy` (KDE Plasma, GNOME). Портал сам сохраняет снимок в файл и отдаёт ссылку;
//! Зырик читает файл в память, затирает его нулями и удаляет. Это нарушает принцип I на
//! мгновение (файл живёт на диске доли секунды) — сказано в README и в `--help`.
//!
//! D-Bus — только сессионная шина по unix-сокету (`tray::bus_is_local`).

use std::collections::HashMap;
use std::io::{Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::time::Duration;

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedValue, Value};

use zyrik::image::Image;
use zyrik::platform::Problem;

const DEST: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";

fn os<E: std::fmt::Display>(e: E) -> Problem {
    Problem::Os(format!("портал: {e}"))
}

/// file:///путь%20с%20пробелами → путь.
fn uri_path(uri: &str) -> Option<PathBuf> {
    let raw = uri.strip_prefix("file://")?;
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?, 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    Some(PathBuf::from(String::from_utf8(out).ok()?))
}

/// Затереть файл нулями и удалить.
fn shred(path: &std::path::Path, len: u64) {
    if let Ok(mut f) = std::fs::OpenOptions::new().write(true).open(path) {
        let zeros = vec![0u8; 64 * 1024];
        let _ = f.seek(SeekFrom::Start(0));
        let mut left = len;
        while left > 0 {
            let n = left.min(zeros.len() as u64) as usize;
            if f.write_all(&zeros[..n]).is_err() {
                break;
            }
            left -= n as u64;
        }
        let _ = f.sync_all();
    }
    let _ = std::fs::remove_file(path);
}

/// PNG → пиксели 0xAARRGGBB.
pub(crate) fn decode(data: &[u8]) -> Result<Image, Problem> {
    let dec = png::Decoder::new(std::io::Cursor::new(data));
    let mut reader = dec.read_info().map_err(os)?;
    let mut buf = zeroize::Zeroizing::new(vec![0; reader.output_buffer_size()]);
    let info = reader.next_frame(&mut buf).map_err(os)?;
    let (w, h) = (info.width, info.height);
    let ch = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        other => return Err(os(format!("цвет {other:?}"))),
    };
    if info.bit_depth != png::BitDepth::Eight {
        return Err(os("PNG не 8 бит"));
    }
    let px = buf[..info.buffer_size()]
        .chunks_exact(ch)
        .map(|p| 0xFF00_0000 | (u32::from(p[0]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[2]))
        .collect();
    Ok(Image::from_pixels(w, h, px))
}

/// Снимок всего экрана без диалога (`interactive: false`).
pub fn screenshot() -> Result<Image, Problem> {
    if !super::tray::bus_is_local() {
        return Err(os("адрес шины D-Bus не unix:"));
    }
    let conn = Connection::session().map_err(os)?;
    let token = format!("zyrik{}", std::process::id());
    let sender = conn.unique_name().ok_or_else(|| os("нет имени на шине"))?.trim_start_matches(':').replace('.', "_");
    let request = format!("{PATH}/request/{sender}/{token}");
    // подписаться на ответ до вызова, иначе можно его пропустить
    let req = Proxy::new(&conn, DEST, request.as_str(), "org.freedesktop.portal.Request").map_err(os)?;
    let mut responses = req.receive_signal("Response").map_err(os)?;
    let portal = Proxy::new(&conn, DEST, PATH, "org.freedesktop.portal.Screenshot").map_err(os)?;
    let mut opts: HashMap<&str, Value> = HashMap::new();
    opts.insert("handle_token", Value::from(token.as_str()));
    opts.insert("interactive", Value::from(false));
    opts.insert("modal", Value::from(false));
    let _: zbus::zvariant::OwnedObjectPath = portal.call("Screenshot", &("", opts)).map_err(os)?;

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(responses.next());
    });
    let msg = rx.recv_timeout(Duration::from_secs(30)).map_err(|_| os("нет ответа"))?.ok_or_else(|| os("нет ответа"))?;
    let (code, results): (u32, HashMap<String, OwnedValue>) = msg.body().deserialize().map_err(os)?;
    if code != 0 {
        return Err(Problem::NoPermission);
    }
    let uri: String = results.get("uri").and_then(|v| String::try_from(v.clone()).ok()).ok_or_else(|| os("нет ссылки на снимок"))?;
    let path = uri_path(&uri).ok_or_else(|| os(format!("ссылка {uri}")))?;
    let data = zeroize::Zeroizing::new(std::fs::read(&path).map_err(os)?);
    shred(&path, data.len() as u64);
    decode(&data)
}

#[cfg(test)]
mod tests {
    use super::uri_path;

    #[test]
    fn file_uris() {
        assert_eq!(uri_path("file:///tmp/a%20b.png").unwrap().to_str(), Some("/tmp/a b.png"));
        assert_eq!(uri_path("file:///home/u/%D0%A1.png").unwrap().to_str(), Some("/home/u/С.png"));
        assert!(uri_path("https://x").is_none());
    }
}
