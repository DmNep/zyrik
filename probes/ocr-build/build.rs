//! Линкуем заранее собранные статические Leptonica и Tesseract (см. README пробы).
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=ZYRIK_OCR_LIB");
    let root = std::env::var_os("ZYRIK_OCR_LIB")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/ocr/install/lib"));
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    // Leptonica под Windows кладёт статический архив с суффиксом .dll — даём ему обычное имя
    for e in std::fs::read_dir(&root).unwrap().flatten() {
        let n = e.file_name().to_string_lossy().to_string();
        if n.starts_with("libleptonica") {
            std::fs::copy(e.path(), out.join("libleptonica.a")).unwrap();
        }
        if n.starts_with("libtesseract") && n.ends_with(".a") {
            std::fs::copy(e.path(), out.join("libtesseract.a")).unwrap();
        }
    }
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=tesseract");
    println!("cargo:rustc-link-lib=static=leptonica");
    let target = std::env::var("TARGET").unwrap();
    if target.contains("windows-gnu") {
        // C++-рантайм MinGW — внутрь exe, никаких libstdc++-6.dll рядом
        for a in ["-Wl,-Bstatic", "-lstdc++", "-lwinpthread", "-lmingwex", "-lgcc_eh", "-lgcc", "-Wl,-Bdynamic", "-lmsvcrt", "-lkernel32"] {
            println!("cargo:rustc-link-arg={a}");
        }
    } else if target.contains("apple") {
        println!("cargo:rustc-link-lib=c++");
    } else {
        println!("cargo:rustc-link-lib=stdc++");
    }
}
