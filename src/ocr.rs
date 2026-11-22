//! Распознавание текста на снимке — для поиска опасного (история 4). Встроенный Tesseract 5
//! (только LSTM) и Leptonica, собранные статически без форматов картинок, curl и libarchive
//! (`tools/ocr-build.sh`). Модели `tessdata_fast` eng и rus вшиты в программу и читаются из
//! памяти (`TessBaseAPIInit5`): ни одного открытия файла и сокета — проверено под strace (R7).
//!
//! Две модели, потому что каждая хорошо читает только свой алфавит: английская — латиницу, цифры
//! и знаки (почти все ключи, токены, адреса), русская — русские метки и имена (R5).
//!
//! Без фичи `ocr` распознавания нет: `available()` → `false`, работает ручная закраска
//! (история 4, сценарий 6).

use crate::image::Image;
use crate::redact::Word;

#[cfg(not(feature = "ocr"))]
pub fn available() -> bool {
    false
}

#[cfg(not(feature = "ocr"))]
pub fn recognize(_frame: &Image) -> Vec<Vec<Word>> {
    Vec::new()
}

#[cfg(feature = "ocr")]
pub fn available() -> bool {
    true
}

/// Слова с рамками в координатах кадра: по набору на каждую модель.
#[cfg(feature = "ocr")]
pub fn recognize(frame: &Image) -> Vec<Vec<Word>> {
    // кадр в RGB — так его ждёт Tesseract; копия затирается
    let mut rgb = zeroize::Zeroizing::new(Vec::with_capacity(frame.pixels().len() * 3));
    for p in frame.pixels() {
        rgb.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8]);
    }
    // модели независимы — распознаём параллельно (у каждой свой TessBaseAPI)
    let (w, h) = (frame.width(), frame.height());
    let rgb: &[u8] = &rgb;
    std::thread::scope(|s| {
        let runs: Vec<_> = ffi::MODELS.iter().map(|(lang, model)| s.spawn(move || ffi::words(model, lang, rgb, w, h))).collect();
        runs.into_iter().map(|r| r.join().unwrap_or_default()).collect()
    })
}

#[cfg(feature = "ocr")]
mod ffi {
    use std::ffi::{c_char, c_int, CStr, CString};

    use crate::redact::Word;
    use crate::session::Rect;

    pub static MODELS: [(&str, &[u8]); 2] = [
        ("eng", include_bytes!("../assets/tessdata/eng.traineddata")),
        ("rus", include_bytes!("../assets/tessdata/rus.traineddata")),
    ];

    #[repr(C)]
    struct Api {
        _p: [u8; 0],
    }
    #[repr(C)]
    struct ResultIt {
        _p: [u8; 0],
    }
    #[repr(C)]
    struct PageIt {
        _p: [u8; 0],
    }

    const OEM_LSTM_ONLY: c_int = 1;
    const RIL_TEXTLINE: c_int = 2;
    const RIL_WORD: c_int = 3;
    /// Leptonica: не печатать ничего (L_SEVERITY_NONE)
    const SEVERITY_NONE: i32 = 6;

    extern "C" {
        fn setMsgSeverity(sev: i32) -> i32;
        fn TessBaseAPICreate() -> *mut Api;
        fn TessBaseAPIDelete(h: *mut Api);
        fn TessBaseAPIInit5(
            h: *mut Api,
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
        fn TessBaseAPISetImage(h: *mut Api, data: *const u8, w: c_int, h_: c_int, bpp: c_int, bpl: c_int);
        fn TessBaseAPISetSourceResolution(h: *mut Api, ppi: c_int);
        fn TessBaseAPIRecognize(h: *mut Api, monitor: *mut u8) -> c_int;
        fn TessBaseAPIClear(h: *mut Api);
        fn TessBaseAPIGetIterator(h: *mut Api) -> *mut ResultIt;
        fn TessResultIteratorGetPageIterator(it: *mut ResultIt) -> *mut PageIt;
        fn TessResultIteratorGetUTF8Text(it: *const ResultIt, level: c_int) -> *mut c_char;
        fn TessResultIteratorNext(it: *mut ResultIt, level: c_int) -> c_int;
        fn TessResultIteratorDelete(it: *mut ResultIt);
        fn TessPageIteratorBoundingBox(it: *const PageIt, level: c_int, l: *mut c_int, t: *mut c_int, r: *mut c_int, b: *mut c_int) -> c_int;
        fn TessPageIteratorIsAtBeginningOf(it: *const PageIt, level: c_int) -> c_int;
        fn TessDeleteText(t: *const c_char);
    }

    /// Одна модель по всему кадру → слова с рамками и номерами строк в порядке чтения.
    pub fn words(model: &[u8], lang: &str, rgb: &[u8], w: u32, h: u32) -> Vec<Word> {
        let mut out = Vec::new();
        let Ok(lang) = CString::new(lang) else { return out };
        unsafe {
            setMsgSeverity(SEVERITY_NONE);
            let api = TessBaseAPICreate();
            if api.is_null() {
                return out;
            }
            let null = std::ptr::null_mut();
            let r = TessBaseAPIInit5(api, model.as_ptr().cast(), model.len() as c_int, lang.as_ptr(), OEM_LSTM_ONLY, null, 0, null, null, 0, 0);
            if r != 0 {
                TessBaseAPIDelete(api);
                return out;
            }
            TessBaseAPISetImage(api, rgb.as_ptr(), w as c_int, h as c_int, 3, (w * 3) as c_int);
            TessBaseAPISetSourceResolution(api, 96);
            if TessBaseAPIRecognize(api, null.cast()) == 0 {
                let it = TessBaseAPIGetIterator(api);
                if !it.is_null() {
                    let pit = TessResultIteratorGetPageIterator(it);
                    let mut line = 0usize;
                    let mut first = true;
                    loop {
                        if TessPageIteratorIsAtBeginningOf(pit, RIL_TEXTLINE) != 0 && !first {
                            line += 1;
                        }
                        first = false;
                        let t = TessResultIteratorGetUTF8Text(it, RIL_WORD);
                        if !t.is_null() {
                            let text = CStr::from_ptr(t).to_string_lossy().into_owned();
                            TessDeleteText(t);
                            let (mut l, mut tp, mut r, mut b) = (0, 0, 0, 0);
                            TessPageIteratorBoundingBox(pit, RIL_WORD, &mut l, &mut tp, &mut r, &mut b);
                            if !text.trim().is_empty() {
                                out.push(Word { text, rect: Rect { x: l, y: tp, w: r - l, h: b - tp }, line });
                            }
                        }
                        if TessResultIteratorNext(it, RIL_WORD) == 0 {
                            break;
                        }
                    }
                    TessResultIteratorDelete(it);
                }
            }
            // Clear отпускает картинку и результаты внутри Tesseract
            TessBaseAPIClear(api);
            TessBaseAPIDelete(api);
        }
        out
    }
}
