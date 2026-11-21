//! Сохранение в файл (история 3): PNG без метаданных, имя по дате-времени, без перезаписи.
//! Это единственное место, где Зырик пишет на диск, и только по явной команде (FR-006, FR-007).

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use zeroize::Zeroizing;

use crate::image::Image;

/// Местные дата и время — их даёт платформа (в std часового пояса нет).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

/// `Зырик_2026-11-20_14-05-33.png`
pub fn file_name(prefix: &str, t: Stamp) -> String {
    format!(
        "{prefix}_{:04}-{:02}-{:02}_{:02}-{:02}-{:02}.png",
        t.year, t.month, t.day, t.hour, t.minute, t.second
    )
}

/// PNG: только IHDR, IDAT, IEND — ни времени, ни программы, ни прочих текстовых полей.
/// Байты затираются при удалении: это тоже копия снимка.
pub fn encode_png(img: &Image) -> Zeroizing<Vec<u8>> {
    let (w, h) = (img.width(), img.height());
    let mut rgb = Zeroizing::new(Vec::with_capacity(w as usize * h as usize * 3));
    for p in img.pixels() {
        rgb.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8]);
    }
    let mut out = Zeroizing::new(Vec::new());
    {
        let mut enc = png::Encoder::new(&mut *out, w, h);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Default);
        let mut wr = enc.write_header().expect("PNG в память не падает");
        wr.write_image_data(&rgb).expect("PNG в память не падает");
        wr.finish().expect("PNG в память не падает");
    }
    out
}

/// Записать в `dir`, не трогая существующие файлы: `имя.png`, `имя_2.png`, `имя_3.png`…
/// Папку не создаёт: если её нет — ошибка, и тогда приложение предложит диалог.
pub fn save_unique(dir: &Path, name: &str, data: &[u8]) -> io::Result<PathBuf> {
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) => (s, format!(".{e}")),
        None => (name, String::new()),
    };
    for n in 1..=999 {
        let file = if n == 1 { name.to_string() } else { format!("{stem}_{n}{ext}") };
        let path = dir.join(file);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut f) => {
                if let Err(e) = f.write_all(data).and_then(|()| f.sync_all()) {
                    drop(f);
                    let _ = std::fs::remove_file(&path);
                    return Err(e);
                }
                return Ok(path);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists, "999 файлов с таким именем"))
}

/// Записать по пути, который человек выбрал в диалоге (перезапись он уже подтвердил).
pub fn save_to(path: &Path, data: &[u8]) -> io::Result<()> {
    std::fs::write(path, data)
}
