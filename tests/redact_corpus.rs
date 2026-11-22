//! T071 / SC-006, SC-008: полнота и ложные срабатывания на корпусе снимков настоящих xterm и
//! Chromium (`tools/corpus`). Эталон — рамки секретов по точному цвету пикселей.
//!   cargo test --release --features ocr --test redact_corpus -- --nocapture

#![cfg(feature = "ocr")]

use std::path::PathBuf;

use regex::Regex;
use zyrik::image::Image;
use zyrik::redact::{detect_all, Category, Finding};
use zyrik::session::Rect;

/// Секрет найден, если находки накрывают не меньше этой доли его пикселей-рамок.
const COVER: f64 = 0.6;

struct Item {
    category: String,
    rects: Vec<Rect>,
}

fn load_png(path: &PathBuf) -> Image {
    let data = std::fs::read(path).expect("png");
    let dec = png::Decoder::new(std::io::Cursor::new(&data[..]));
    let mut r = dec.read_info().expect("png");
    let mut buf = vec![0; r.output_buffer_size()];
    let info = r.next_frame(&mut buf).expect("кадр");
    let ch = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        other => panic!("{other:?}"),
    };
    let px = buf[..info.buffer_size()]
        .chunks_exact(ch)
        .map(|p| 0xFF00_0000 | (u32::from(p[0]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[2]))
        .collect();
    Image::from_pixels(info.width, info.height, px)
}

/// Эталон без serde: категория и рамки из JSON генератора.
fn load_items(path: &PathBuf) -> Vec<Item> {
    let text = std::fs::read_to_string(path).expect("json");
    let item = Regex::new(r#""category":\s*"(\w+)",\s*"rects":\s*\[((?:\s*\[[\d,\s]+\],?)*)\s*\]"#).unwrap();
    let rect = Regex::new(r"\[\s*(\d+),\s*(\d+),\s*(\d+),\s*(\d+)\s*\]").unwrap();
    item.captures_iter(&text)
        .map(|c| Item {
            category: c[1].to_string(),
            rects: rect
                .captures_iter(&c[2])
                .map(|r| Rect { x: r[1].parse().unwrap(), y: r[2].parse().unwrap(), w: r[3].parse().unwrap(), h: r[4].parse().unwrap() })
                .collect(),
        })
        .collect()
}

fn covered(item: &Item, found: &[Finding]) -> f64 {
    let (mut all, mut hit) = (0u64, 0u64);
    for r in &item.rects {
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                all += 1;
                if found.iter().any(|f| f.contains(x, y)) {
                    hit += 1;
                }
            }
        }
    }
    if all == 0 { 0.0 } else { hit as f64 / all as f64 }
}

fn touches(f: &Finding, items: &[Item]) -> bool {
    f.rects.iter().any(|a| items.iter().flat_map(|i| &i.rects).any(|b| a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()))
}

/// Категории с контрольной суммой: тут нужна полнота ≥ 99% (SC-006).
fn checksummed(c: &str) -> bool {
    matches!(c, "Card" | "Snils" | "Inn")
}

#[test]
fn corpus_recall_and_false_positives() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/redact_corpus");
    let mut names: Vec<PathBuf> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "png")).collect();
    names.sort();
    assert!(names.len() >= 30, "в корпусе меньше 30 снимков");
    let (mut total, mut found_n, mut cat_ok, mut chk_total, mut chk_found) = (0, 0, 0, 0, 0);
    let mut fp_total = 0;
    let mut worst_fp = 0;
    let mut misses = Vec::new();
    for png in &names {
        let items = load_items(&png.with_extension("json"));
        let img = load_png(png);
        let found = detect_all(&zyrik::ocr::recognize(&img));
        let name = png.file_stem().unwrap().to_string_lossy();
        let mut line = format!("{name:<26}");
        for it in &items {
            total += 1;
            let c = covered(it, &found);
            let ok = c >= COVER;
            if checksummed(&it.category) {
                chk_total += 1;
                chk_found += usize::from(ok);
            }
            if ok {
                found_n += 1;
                let same = found.iter().any(|f| format!("{:?}", f.category) == it.category && touches(f, std::slice::from_ref(it)));
                cat_ok += usize::from(same);
                line += &format!(" ✓{}", it.category);
            } else {
                line += &format!(" ✗{}({:.0}%)", it.category, c * 100.0);
                misses.push(format!("{name}: {}", it.category));
            }
        }
        let fp: Vec<Category> = found.iter().filter(|f| !touches(f, &items)).map(|f| f.category).collect();
        fp_total += fp.len();
        worst_fp = worst_fp.max(fp.len());
        if !fp.is_empty() {
            line += &format!("  ложные: {fp:?}");
        }
        println!("{line}");
    }
    let recall = found_n as f64 / total as f64;
    println!(
        "\nснимков {}, секретов {total}: найдено {found_n} ({:.1}%), категория верна у {cat_ok}; с контрольной суммой {chk_found}/{chk_total}; ложных всего {fp_total}, на снимок не больше {worst_fp}",
        names.len(),
        recall * 100.0
    );
    assert!(recall >= 0.90, "SC-006: полнота {:.1}% < 90%; пропущены: {misses:?}", recall * 100.0);
    assert!(chk_found * 100 >= chk_total * 99, "SC-006: категории с контрольной суммой {chk_found}/{chk_total} < 99%");
    assert!(worst_fp <= 1, "SC-008: на одном снимке {worst_fp} ложных срабатываний");
}
