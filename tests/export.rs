//! T039: PNG без метаданных, имя по дате-времени, без перезаписи.

use zyrik::export::{encode_png, file_name, save_unique, Stamp};
use zyrik::image::Image;

fn chunks(png: &[u8]) -> Vec<String> {
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let mut out = Vec::new();
    let mut i = 8;
    while i + 8 <= png.len() {
        let len = u32::from_be_bytes(png[i..i + 4].try_into().unwrap()) as usize;
        out.push(String::from_utf8_lossy(&png[i + 4..i + 8]).to_string());
        i += 12 + len;
    }
    assert_eq!(i, png.len(), "ровно до конца файла");
    out
}

fn img() -> Image {
    // 0xAARRGGBB: красный, зелёный, синий, белый
    Image::from_pixels(2, 2, vec![0xFFFF_0000, 0xFF00_FF00, 0xFF00_00FF, 0xFFFF_FFFF])
}

#[test]
fn png_has_only_ihdr_idat_iend() {
    let png = encode_png(&img());
    let c = chunks(&png);
    assert_eq!(c.first().map(String::as_str), Some("IHDR"));
    assert_eq!(c.last().map(String::as_str), Some("IEND"));
    assert!(c.iter().all(|n| n == "IHDR" || n == "IDAT" || n == "IEND"), "{c:?}");
}

#[test]
fn png_pixels_round_trip() {
    let png = encode_png(&img());
    let dec = png::Decoder::new(std::io::Cursor::new(&png[..]));
    let mut r = dec.read_info().unwrap();
    let mut buf = vec![0; r.output_buffer_size()];
    let info = r.next_frame(&mut buf).unwrap();
    assert_eq!((info.width, info.height), (2, 2));
    assert_eq!(info.color_type, png::ColorType::Rgb);
    assert_eq!(&buf[..12], [255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255]);
}

#[test]
fn name_from_date_time() {
    let t = Stamp { year: 2026, month: 11, day: 20, hour: 14, minute: 5, second: 33 };
    assert_eq!(file_name("Зырик", t), "Зырик_2026-11-20_14-05-33.png");
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("zyrik-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn never_overwrites_adds_suffix() {
    let d = temp_dir("suffix");
    let a = save_unique(&d, "Зырик_2026-11-20_14-05-33.png", b"one").unwrap();
    let b = save_unique(&d, "Зырик_2026-11-20_14-05-33.png", b"two").unwrap();
    let c = save_unique(&d, "Зырик_2026-11-20_14-05-33.png", b"three").unwrap();
    assert_eq!(a.file_name().unwrap(), "Зырик_2026-11-20_14-05-33.png");
    assert_eq!(b.file_name().unwrap(), "Зырик_2026-11-20_14-05-33_2.png");
    assert_eq!(c.file_name().unwrap(), "Зырик_2026-11-20_14-05-33_3.png");
    assert_eq!(std::fs::read(&a).unwrap(), b"one", "первый файл не тронут");
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn missing_folder_is_an_error_not_a_new_folder() {
    let d = temp_dir("missing").join("нет-такой-папки");
    assert!(save_unique(&d, "x.png", b"x").is_err());
    assert!(!d.exists(), "папки сами не создаём");
}
