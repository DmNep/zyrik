//! Проба R4/R5/R7: встроенный Tesseract распознаёт нарисованный «скриншот» с моделью из памяти.
//! Печатает время, распознанные строки и что нашёл `redact::detect`.
//!   ocr-probe [eng|rus|both] [scale]

use std::ffi::{c_char, c_int, CStr};
use std::time::Instant;

use zyrik::annot::text;
use zyrik::image::Image;
use zyrik::redact::{detect, Word};
use zyrik::session::Rect;

#[repr(C)]
struct TessBaseAPI {
    _p: [u8; 0],
}
#[repr(C)]
struct TessResultIterator {
    _p: [u8; 0],
}
#[repr(C)]
struct TessPageIterator {
    _p: [u8; 0],
}

const OEM_LSTM_ONLY: c_int = 1;
const RIL_TEXTLINE: c_int = 2;
const RIL_WORD: c_int = 3;

extern "C" {
    fn TessBaseAPICreate() -> *mut TessBaseAPI;
    fn TessBaseAPIDelete(h: *mut TessBaseAPI);
    fn TessBaseAPIInit5(
        h: *mut TessBaseAPI,
        data: *const c_char,
        data_size: c_int,
        language: *const c_char,
        oem: c_int,
        configs: *mut *mut c_char,
        configs_size: c_int,
        vars_vec: *mut *mut c_char,
        vars_values: *mut *mut c_char,
        vars_vec_size: usize,
        set_only_non_debug_params: c_int,
    ) -> c_int;
    fn TessBaseAPISetImage(h: *mut TessBaseAPI, data: *const u8, w: c_int, h_: c_int, bpp: c_int, bpl: c_int);
    fn TessBaseAPISetSourceResolution(h: *mut TessBaseAPI, ppi: c_int);
    fn TessBaseAPISetVariable(h: *mut TessBaseAPI, name: *const c_char, value: *const c_char) -> c_int;
    fn TessBaseAPIRecognize(h: *mut TessBaseAPI, monitor: *mut u8) -> c_int;
    fn TessBaseAPIGetIterator(h: *mut TessBaseAPI) -> *mut TessResultIterator;
    fn TessResultIteratorGetPageIterator(it: *mut TessResultIterator) -> *mut TessPageIterator;
    fn TessResultIteratorGetUTF8Text(it: *const TessResultIterator, level: c_int) -> *mut c_char;
    fn TessResultIteratorNext(it: *mut TessResultIterator, level: c_int) -> c_int;
    fn TessResultIteratorDelete(it: *mut TessResultIterator);
    fn TessPageIteratorBoundingBox(it: *const TessPageIterator, level: c_int, l: *mut c_int, t: *mut c_int, r: *mut c_int, b: *mut c_int) -> c_int;
    fn TessPageIteratorIsAtBeginningOf(it: *const TessPageIterator, level: c_int) -> c_int;
    fn TessDeleteText(t: *const c_char);
}

static ENG: &[u8] = include_bytes!("../../../assets/tessdata/eng.traineddata");
static RUS: &[u8] = include_bytes!("../../../assets/tessdata/rus.traineddata");

const LINES: &[&str] = &[
    "Пишите на ivan.petrov@example.com или звоните +7 (912) 345-67-89",
    "Карта 4111 1111 1111 1111 до 12/27, СНИЛС 112-233-445 95",
    "export DB_PASSWORD=hunter2secret",
    "git clone https://deploy:Zk8qL2mX9vB4nR7t@git.corp/app.git",
    "ssh root@10.20.30.40 -i C:\\Users\\ivanov\\.ssh\\id_rsa",
    "1. abandon 2. ability 3. able 4. about 5. above 6. absent",
    "7. absorb 8. abstract 9. absurd 10. abuse 11. access 12. accident",
    "fn main() { let v: Vec<u32> = Vec::new(); println!(\"{}\", v.len()); }",
    "send 0.5 ETH to 0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed",
];

/// «Скриншот»: тёмный текст на светлом, шрифт подписей Зырика.
fn screenshot(size: f32) -> Image {
    let (w, lh) = (1400u32, (size * 1.6) as u32);
    let h = lh * LINES.len() as u32 + 40;
    let mut img = Image::from_pixels(w, h, vec![0xFFF5_F5F5; (w * h) as usize]);
    let clip = Rect { x: 0, y: 0, w: w as i32, h: h as i32 };
    for (i, l) in LINES.iter().enumerate() {
        text::draw(&mut img, 20, 20 + (i as u32 * lh) as i32, l, size, 0xFF20_2020, clip);
    }
    img
}

fn ocr(model: &[u8], lang: &str, img: &Image, scale: u32) -> (Vec<Word>, f64) {
    // RGB, при scale > 1 — ближайший пиксель (мелкий текст крупнее)
    let (w, h) = (img.width() * scale, img.height() * scale);
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            let p = img.get(x / scale, y / scale);
            rgb.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
        }
    }
    let t0 = Instant::now();
    let mut words = Vec::new();
    unsafe {
        let api = TessBaseAPICreate();
        let lang_c = std::ffi::CString::new(lang).unwrap();
        let r = TessBaseAPIInit5(api, model.as_ptr().cast(), model.len() as c_int, lang_c.as_ptr(), OEM_LSTM_ONLY, std::ptr::null_mut(), 0, std::ptr::null_mut(), std::ptr::null_mut(), 0, 0);
        assert_eq!(r, 0, "Init5");
        TessBaseAPISetVariable(api, c"debug_file".as_ptr(), c"".as_ptr());
        TessBaseAPISetImage(api, rgb.as_ptr(), w as c_int, h as c_int, 3, (w * 3) as c_int);
        TessBaseAPISetSourceResolution(api, 96 * scale as c_int);
        assert_eq!(TessBaseAPIRecognize(api, std::ptr::null_mut()), 0);
        let it = TessBaseAPIGetIterator(api);
        let pit = TessResultIteratorGetPageIterator(it);
        let mut line = 0usize;
        let mut first = true;
        if !it.is_null() {
            loop {
                if TessPageIteratorIsAtBeginningOf(pit, RIL_TEXTLINE) != 0 && !first {
                    line += 1;
                }
                first = false;
                let t = TessResultIteratorGetUTF8Text(it, RIL_WORD);
                if !t.is_null() {
                    let s = CStr::from_ptr(t).to_string_lossy().to_string();
                    TessDeleteText(t);
                    let (mut l, mut tp, mut r, mut b) = (0, 0, 0, 0);
                    TessPageIteratorBoundingBox(pit, RIL_WORD, &mut l, &mut tp, &mut r, &mut b);
                    let k = scale as i32;
                    words.push(Word { text: s, rect: Rect { x: l / k, y: tp / k, w: (r - l) / k, h: (b - tp) / k }, line });
                }
                if TessResultIteratorNext(it, RIL_WORD) == 0 {
                    break;
                }
            }
            TessResultIteratorDelete(it);
        }
        TessBaseAPIDelete(api);
    }
    (words, t0.elapsed().as_secs_f64())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let which = args.get(1).map(String::as_str).unwrap_or("both");
    let scale: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1);
    let size: f32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(16.0);
    let img = screenshot(size);
    let runs: Vec<(&str, &[u8])> = match which {
        "eng" => vec![("eng", ENG)],
        "rus" => vec![("rus", RUS)],
        _ => vec![("eng", ENG), ("rus", RUS)],
    };
    for (lang, model) in runs {
        let (words, secs) = ocr(model, lang, &img, scale);
        println!("=== {lang}, ×{scale}, шрифт {size}px: {:.2} с, слов {}", secs, words.len());
        let maxl = words.iter().map(|w| w.line).max().unwrap_or(0);
        for l in 0..=maxl {
            let s: Vec<&str> = words.iter().filter(|w| w.line == l).map(|w| w.text.as_str()).collect();
            println!("  {}", s.join(" "));
        }
        for f in detect(&words) {
            println!("  → {:?} {:?}", f.category, f.rects.first());
        }
    }
}
