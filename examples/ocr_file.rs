//! Отладка распознавания: PNG-файл → прочитанные строки обеих моделей и находки.
//! Не часть Зырика (печатает распознанный текст — только для своих тестовых картинок).
//!   cargo run --release --features ocr --example ocr_file -- снимок.png

#[cfg(feature = "ocr")]
fn main() {
    let path = std::env::args().nth(1).expect("путь к PNG");
    let data = std::fs::read(&path).expect("файл");
    let dec = png::Decoder::new(std::io::Cursor::new(&data[..]));
    let mut r = dec.read_info().expect("PNG");
    let mut buf = vec![0; r.output_buffer_size()];
    let info = r.next_frame(&mut buf).expect("кадр");
    let ch = if info.color_type == png::ColorType::Rgba { 4 } else { 3 };
    let px = buf[..info.buffer_size()].chunks_exact(ch).map(|p| 0xFF00_0000 | (u32::from(p[0]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[2])).collect();
    let img = zyrik::image::Image::from_pixels(info.width, info.height, px);
    let t0 = std::time::Instant::now();
    let sets = zyrik::ocr::recognize(&img);
    println!("распознано за {:?}", t0.elapsed());
    for (name, words) in ["eng", "rus"].iter().zip(&sets) {
        println!("=== {name}");
        let maxl = words.iter().map(|w| w.line).max().unwrap_or(0);
        for l in 0..=maxl {
            let s: Vec<&str> = words.iter().filter(|w| w.line == l).map(|w| w.text.as_str()).collect();
            if !s.is_empty() {
                println!("  {}", s.join(" "));
            }
        }
    }
    for f in zyrik::redact::detect_all(&sets) {
        println!("→ {:?} {:?}", f.category, f.rects);
    }
}

#[cfg(not(feature = "ocr"))]
fn main() {}
