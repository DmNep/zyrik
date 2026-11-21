//! 1. Языковые таблицы `locales/*.toml` → `enum Msg` и массивы строк (D6).
//!    Пропущенный ключ или несовпадающие подстановки — ошибка сборки.
//! 2. Windows: значок и сведения о версии в exe (как в Зябре).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    println!("cargo:rerun-if-changed=locales");
    locales(&out);
    println!("cargo:rerun-if-changed=assets/zyrik.ico");
    embed_resources(&out);
}

// ---------- языки ----------

fn locales(out: &Path) {
    let mut tables = BTreeMap::new();
    let mut files: Vec<_> = std::fs::read_dir("locales")
        .expect("папка locales")
        .map(|e| e.expect("locales").path())
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .collect();
    files.sort();
    for path in &files {
        println!("cargo:rerun-if-changed={}", path.display());
        let code = path.file_stem().unwrap().to_string_lossy().to_string();
        let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let table = parse_table(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        tables.insert(code, table);
    }
    let base = tables.get("en").expect("нужна таблица locales/en.toml — запасной язык");
    let keys: Vec<&String> = base.keys().collect();

    let mut problems = Vec::new();
    for (code, table) in &tables {
        for k in &keys {
            match table.get(*k) {
                None => problems.push(format!("{code}.toml: нет ключа \"{k}\"")),
                Some(v) if v.trim().is_empty() => problems.push(format!("{code}.toml: пустой \"{k}\"")),
                Some(v) => {
                    let (a, b) = (placeholders(&base[*k]), placeholders(v));
                    if a != b {
                        problems.push(format!("{code}.toml: \"{k}\" подстановки {b:?}, а в en {a:?}"));
                    }
                }
            }
        }
        for k in table.keys() {
            if !base.contains_key(k) {
                problems.push(format!("{code}.toml: лишний ключ \"{k}\" (нет в en.toml)"));
            }
        }
    }
    if !problems.is_empty() {
        panic!("языковые таблицы не сходятся:\n  {}", problems.join("\n  "));
    }

    let mut code = String::new();
    code.push_str("/// Все строки интерфейса. Сгенерировано build.rs из locales/*.toml.\n");
    code.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq)]\n#[repr(usize)]\npub enum Msg {\n");
    for k in &keys {
        writeln!(code, "    /// `{k}`\n    {},", variant(k)).unwrap();
    }
    code.push_str("}\n\n");
    writeln!(code, "pub const MSG_COUNT: usize = {};", keys.len()).unwrap();
    writeln!(code, "pub const MSG_KEYS: [&str; MSG_COUNT] = {:?};", keys).unwrap();
    writeln!(code, "pub static ALL_MSGS: [Msg; MSG_COUNT] = [{}];",
        keys.iter().map(|k| format!("Msg::{}", variant(k))).collect::<Vec<_>>().join(", ")).unwrap();
    code.push_str("pub static LOCALES: &[Locale] = &[\n");
    for (c, table) in &tables {
        let strings: Vec<&String> = keys.iter().map(|k| &table[*k]).collect();
        writeln!(code, "    Locale {{ code: {c:?}, strings: &{strings:?} }},").unwrap();
    }
    code.push_str("];\n");
    std::fs::write(out.join("i18n.rs"), code).expect("write i18n.rs");
}

/// `tray.capture_later` → `TrayCaptureLater`
fn variant(key: &str) -> String {
    key.split(['.', '_'])
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_ascii_uppercase().to_string() + c.as_str()).unwrap_or_default()
        })
        .collect()
}

fn placeholders(s: &str) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    let mut rest = s;
    while let Some(i) = rest.find('{') {
        rest = &rest[i + 1..];
        if let Some(j) = rest.find('}') {
            let name = &rest[..j];
            if !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                set.insert(name.to_string());
            }
            rest = &rest[j + 1..];
        }
    }
    set
}

/// Подмножество TOML: `"ключ" = "строка"` и `"ключ" = """многострочная"""`, комментарии `#`.
fn parse_table(text: &str) -> Result<BTreeMap<String, String>, String> {
    let mut map = BTreeMap::new();
    let mut lines = text.lines().enumerate();
    while let Some((n, line)) = lines.next() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let at = |msg: &str| format!("строка {}: {msg}", n + 1);
        let (key, value) = line.split_once('=').ok_or_else(|| at("нет «=»"))?;
        let key = key.trim().trim_matches('"').to_string();
        if key.is_empty() || !key.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'_') {
            return Err(at("ключ — только a-z, 0-9, «.» и «_»"));
        }
        let value = value.trim();
        let raw = if let Some(first) = value.strip_prefix("\"\"\"") {
            // многострочная: первая пустая строка после """ отбрасывается, как в TOML
            let mut buf = String::new();
            let mut cur = first.to_string();
            let mut started = !first.is_empty();
            loop {
                if let Some(end) = cur.find("\"\"\"") {
                    buf.push_str(&cur[..end]);
                    break;
                }
                if started {
                    buf.push_str(&cur);
                    buf.push('\n');
                }
                started = true;
                cur = lines.next().ok_or_else(|| at("не закрыта \"\"\""))?.1.to_string();
            }
            buf
        } else {
            let inner = value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .ok_or_else(|| at("значение — строка в кавычках"))?;
            inner.to_string()
        };
        let value = unescape(&raw).map_err(|e| at(&e))?;
        if map.insert(key.clone(), value).is_some() {
            return Err(at(&format!("ключ \"{key}\" повторяется")));
        }
    }
    Ok(map)
}

fn unescape(s: &str) -> Result<String, String> {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            other => return Err(format!("непонятная последовательность \\{}", other.map(String::from).unwrap_or_default())),
        }
    }
    Ok(out)
}

// ---------- значок и версия для Windows ----------

fn embed_resources(out: &Path) {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("gnu") {
        println!("cargo:warning=значок и версия вшиваются только при сборке GNU-тулчейном (windres)");
        return;
    }
    let version = std::env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION");
    let mut parts = version.split('.');
    let (major, minor, patch) = (
        parts.next().unwrap_or("0"),
        parts.next().unwrap_or("0"),
        parts.next().unwrap_or("0"),
    );
    std::fs::copy("assets/zyrik.ico", out.join("zyrik.ico")).expect("copy icon");
    let rc = format!(
        r#"#pragma code_page(65001)
1 ICON "zyrik.ico"
1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEFLAGSMASK 0x3fL
FILEFLAGS 0
FILEOS 0x40004L
FILETYPE 0x1L
FILESUBTYPE 0
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "FileDescription", "Зырик {version}"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "zyrik"
      VALUE "LegalCopyright", "MIT"
      VALUE "OriginalFilename", "zyrik-{version}.exe"
      VALUE "ProductName", "Зырик"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    );
    let rc_path = out.join("zyrik.rc");
    let obj = out.join("zyrik-res.o");
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend(rc.into_bytes());
    std::fs::write(&rc_path, bytes).expect("write rc");
    let status = std::process::Command::new("windres")
        .current_dir(out)
        .args(["--codepage", "65001", "-O", "coff", "-i"])
        .arg(&rc_path)
        .arg("-o")
        .arg(&obj)
        .status()
        .unwrap_or_else(|e| panic!("windres: {e}"));
    assert!(status.success(), "windres failed");
    println!("cargo:rustc-link-arg-bins={}", obj.display());
}
