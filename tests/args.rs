//! T009: параметры запуска из истории 8.

use std::path::PathBuf;
use zyrik::args::{parse, ArgError, Launch, Options};
use zyrik::hotkey::{Hotkey, Key, KeyError, Mods};

fn run(args: &[&str]) -> Options {
    match parse(args.iter().copied()) {
        Ok(Launch::Run(o)) => o,
        other => panic!("{args:?} → {other:?}"),
    }
}

fn err(args: &[&str]) -> ArgError {
    match parse(args.iter().copied()) {
        Err(e) => e,
        other => panic!("{args:?} должно быть ошибкой, а вышло {other:?}"),
    }
}

const CTRL: Mods = Mods { ctrl: true, shift: false, alt: false, meta: false };
const SHIFT: Mods = Mods { ctrl: false, shift: true, alt: false, meta: false };

#[test]
fn defaults_without_arguments() {
    let o = run(&[]);
    assert_eq!(o.key, Hotkey::default_capture());
    assert_eq!(o.key_screen, None);
    assert_eq!(o.key_window, None);
    assert_eq!(o.save_dir, None);
    assert_eq!(o.clear_clipboard, None);
    assert!(o.detect);
    assert_eq!(o.lang, None);
}

#[cfg(not(target_os = "macos"))]
#[test]
fn default_key_is_ctrl_prtscr() {
    assert_eq!(Hotkey::default_capture(), Hotkey::new(CTRL, Key::PrintScreen));
    assert_eq!(Hotkey::default_capture().to_string(), "Ctrl+PrtScr");
}

#[test]
fn every_option() {
    let o = run(&[
        "--key", "Shift+PrtScr",
        "--key-screen", "ctrl+alt+f12",
        "--key-window=Ctrl+Shift+W",
        "--save-dir", r"D:\Скрины",
        "--clear-clipboard", "5",
        "--no-detect",
        "--lang", "EN",
    ]);
    assert_eq!(o.key, Hotkey::new(SHIFT, Key::PrintScreen));
    assert_eq!(o.key_screen, Some(Hotkey::new(Mods { ctrl: true, alt: true, ..Mods::default() }, Key::F(12))));
    assert_eq!(o.key_window, Some(Hotkey::new(Mods { ctrl: true, shift: true, ..Mods::default() }, Key::Letter('W'))));
    assert_eq!(o.save_dir, Some(PathBuf::from(r"D:\Скрины")));
    assert_eq!(o.clear_clipboard, Some(5));
    assert!(!o.detect);
    assert_eq!(o.lang.as_deref(), Some("en"));
}

#[test]
fn help_in_any_spelling() {
    for a in ["--help", "-h", "/?", "help"] {
        assert_eq!(parse([a]), Ok(Launch::Help), "{a}");
    }
    // справка побеждает даже рядом с ошибкой: человек просит помощи
    assert_eq!(parse(["--bogus", "--help"]), Ok(Launch::Help));
}

#[test]
fn unknown_option_is_an_error() {
    assert_eq!(err(&["--keyy", "F1"]), ArgError::Unknown("--keyy".into()));
    assert_eq!(err(&["somefile.png"]), ArgError::Unknown("somefile.png".into()));
}

#[test]
fn missing_values() {
    assert_eq!(err(&["--key"]), ArgError::MissingValue("--key"));
    assert_eq!(err(&["--save-dir"]), ArgError::MissingValue("--save-dir"));
    assert_eq!(err(&["--lang"]), ArgError::MissingValue("--lang"));
    // следующий параметр — не значение
    assert_eq!(err(&["--key", "--no-detect"]), ArgError::MissingValue("--key"));
    assert_eq!(err(&["--key="]), ArgError::MissingValue("--key"));
}

#[test]
fn repeated_option() {
    assert_eq!(err(&["--key", "F1", "--key", "F2"]), ArgError::Duplicate("--key"));
    assert_eq!(err(&["--no-detect", "--no-detect"]), ArgError::Duplicate("--no-detect"));
}

#[test]
fn bad_key_combinations() {
    let bad = |v: &str| match err(&["--key", v]) {
        ArgError::BadKey { option: "--key", value, reason } => {
            assert_eq!(value, v);
            reason
        }
        e => panic!("{v}: {e:?}"),
    };
    assert_eq!(bad("Ctrl+"), KeyError::Unknown("".into()));
    assert_eq!(bad("Ctrl+Shift"), KeyError::NoKey);
    assert_eq!(bad("Ctrl+A+B"), KeyError::TwoKeys);
    assert_eq!(bad("Ctrl+ctrl+F1"), KeyError::Repeated("ctrl".into()));
    assert_eq!(bad("Ctrl+Hyper"), KeyError::Unknown("Hyper".into()));
    assert_eq!(bad("A"), KeyError::NeedsModifier);
    assert_eq!(bad("F25"), KeyError::Unknown("F25".into()));
    assert_eq!(bad("F0"), KeyError::Unknown("F0".into()));
    assert_eq!(bad(" "), KeyError::Empty);
}

#[test]
fn key_spellings() {
    let k = |v: &str| Hotkey::parse(v).unwrap();
    assert_eq!(k("PrintScreen"), Hotkey::new(Mods::default(), Key::PrintScreen));
    assert_eq!(k(" ctrl + prtsc "), Hotkey::new(CTRL, Key::PrintScreen));
    assert_eq!(k("Cmd+Shift+2"), Hotkey::new(Mods { meta: true, shift: true, ..Mods::default() }, Key::Digit(2)));
    assert_eq!(k("Win+Shift+2"), k("Cmd+Shift+2"));
    assert_eq!(k("Pause").to_string(), "Pause");
    assert_eq!(k("shift+ctrl+alt+f24").to_string(), "Ctrl+Alt+Shift+F24");
}

#[test]
fn same_key_twice_is_an_error() {
    assert_eq!(
        err(&["--key", "Ctrl+F1", "--key-screen", "ctrl+f1"]),
        ArgError::SameKey("--key", "--key-screen")
    );
    assert_eq!(
        err(&["--key-screen", "F9", "--key-window", "F9"]),
        ArgError::SameKey("--key-screen", "--key-window")
    );
    // клавиша по умолчанию тоже считается
    let def = Hotkey::default_capture().to_string();
    assert_eq!(err(&["--key-window", &def]), ArgError::SameKey("--key", "--key-window"));
}

#[test]
fn clear_clipboard_minutes() {
    assert_eq!(run(&["--clear-clipboard", "1"]).clear_clipboard, Some(1));
    assert_eq!(run(&["--clear-clipboard=1440"]).clear_clipboard, Some(1440));
    for v in ["0", "1441", "-3", "five", "2.5"] {
        assert_eq!(err(&["--clear-clipboard", v]), ArgError::BadMinutes(v.into()), "{v}");
    }
}

#[test]
fn language() {
    assert_eq!(run(&["--lang", "ru"]).lang.as_deref(), Some("ru"));
    assert_eq!(err(&["--lang", "de"]), ArgError::BadLang("de".into()));
}

#[test]
fn flags_do_not_take_values() {
    assert_eq!(err(&["--no-detect=yes"]), ArgError::Unknown("--no-detect=yes".into()));
}

#[test]
fn lang_hint_reads_lang_even_when_parsing_fails() {
    use zyrik::args::lang_hint;
    assert_eq!(lang_hint(["--bogus", "--lang", "en"]).as_deref(), Some("en"));
    assert_eq!(lang_hint(["--lang=ru", "--key"]).as_deref(), Some("ru"));
    assert_eq!(lang_hint(["--lang", "xx"]), None);
    assert_eq!(lang_hint(["--key", "F1"]), None);
}

#[test]
fn trigger_takes_optional_shot() {
    use zyrik::platform::Shot;
    let t = |a: &[&str]| parse(a.iter().copied()).unwrap();
    assert_eq!(t(&["--trigger"]), Launch::Trigger(Shot::Region));
    assert_eq!(t(&["--trigger", "region"]), Launch::Trigger(Shot::Region));
    assert_eq!(t(&["--trigger", "screen"]), Launch::Trigger(Shot::Screen));
    assert_eq!(t(&["--trigger", "window"]), Launch::Trigger(Shot::Window));
    assert_eq!(err(&["--trigger", "--trigger"]), ArgError::Duplicate("--trigger"));
    assert!(matches!(err(&["--trigger", "moon"]), ArgError::Unknown(_)));
}
