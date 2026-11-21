//! Главный цикл с поддельной платформой: что ядро просит у ОС в ответ на команды.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use zyrik::app::{arg_error_text, help_text, run};
use zyrik::args::{parse, Launch, Options};
use zyrik::hotkey::Hotkey;
use zyrik::i18n::Lang;
use zyrik::platform::{Command, Frame, MenuItem, Platform, Problem, Shot};

#[derive(Default)]
struct Fake {
    already_running: bool,
    busy: Vec<Hotkey>,
    commands: VecDeque<Command>,
    tray: Option<(String, Vec<MenuItem>)>,
    hotkeys: Vec<(Shot, Hotkey)>,
    scheduled: Vec<(Command, u32)>,
    messages: RefCell<Vec<(String, bool)>>,
    log: RefCell<Vec<String>>,
}

impl Platform for Fake {
    fn system_lang(&self) -> Option<String> {
        None
    }
    fn claim_single_instance(&mut self) -> Result<(), Problem> {
        if self.already_running { Err(Problem::AlreadyRunning) } else { Ok(()) }
    }
    fn show_tray(&mut self, tip: &str, menu: &[MenuItem]) -> Result<(), Problem> {
        self.tray = Some((tip.into(), menu.to_vec()));
        Ok(())
    }
    fn set_tray_tip(&mut self, tip: &str) {
        self.tray.as_mut().unwrap().0 = tip.into();
    }
    fn register_hotkey(&mut self, shot: Shot, key: Hotkey) -> Result<(), Problem> {
        if self.busy.contains(&key) {
            return Err(Problem::KeyBusy);
        }
        self.hotkeys.push((shot, key));
        Ok(())
    }
    fn schedule(&mut self, command: Command, delay_ms: u32) {
        self.scheduled.push((command, delay_ms));
    }
    fn next_command(&mut self) -> Option<Command> {
        self.commands.pop_front()
    }
    fn capture(&mut self) -> Result<Frame, Problem> {
        Err(Problem::NotYet("phase 2"))
    }
    fn show_overlay(&mut self, _: &Frame) -> Result<(), Problem> {
        Err(Problem::NotYet("phase 2"))
    }
    fn set_clipboard_image(&mut self, _: u32, _: u32, _: &[u8]) -> Result<(), Problem> {
        Err(Problem::NotYet("phase 2"))
    }
    fn save_dialog(&mut self, _: &Path, _: &str) -> Result<Option<PathBuf>, Problem> {
        Err(Problem::NotYet("phase 4"))
    }
    fn message(&self, _: &str, text: &str, error: bool) {
        self.messages.borrow_mut().push((text.into(), error));
    }
    fn debug_log(&self, text: &str) {
        self.log.borrow_mut().push(text.into());
    }
}

fn opts(args: &[&str]) -> Options {
    match parse(args.iter().copied()).unwrap() {
        Launch::Run(o) => o,
        Launch::Help => unreachable!(),
    }
}

fn ru() -> Lang {
    Lang::from_code("ru").unwrap()
}

fn go(mut fake: Fake, args: &[&str]) -> Fake {
    run(&mut fake, &opts(args), ru());
    fake
}

#[test]
fn second_copy_only_says_so() {
    let f = go(Fake { already_running: true, ..Fake::default() }, &[]);
    assert!(f.tray.is_none());
    assert!(f.hotkeys.is_empty());
    assert_eq!(f.messages.borrow().len(), 1);
    assert!(f.messages.borrow()[0].0.contains("уже запущен"));
    assert!(!f.messages.borrow()[0].1, "не ошибка, а сведение");
}

#[test]
fn tray_menu_and_default_hotkey() {
    let f = go(Fake::default(), &[]);
    let (tip, menu) = f.tray.unwrap();
    assert!(tip.contains(&Hotkey::default_capture().to_string()));
    let texts: Vec<_> = menu.iter().map(|m| m.text.as_str()).collect();
    assert_eq!(texts, ["Зырнуть", "Зырнуть через 3 с", "О программе", "Выход"]);
    assert_eq!(f.hotkeys, [(Shot::Region, Hotkey::default_capture())]);
    assert!(f.messages.borrow().is_empty());
}

#[test]
fn every_configured_hotkey_is_registered() {
    let f = go(Fake::default(), &["--key", "Shift+PrtScr", "--key-screen", "Ctrl+F9", "--key-window", "Alt+F9"]);
    assert_eq!(
        f.hotkeys,
        [
            (Shot::Region, Hotkey::parse("Shift+PrtScr").unwrap()),
            (Shot::Screen, Hotkey::parse("Ctrl+F9").unwrap()),
            (Shot::Window, Hotkey::parse("Alt+F9").unwrap()),
        ]
    );
}

#[test]
fn busy_key_explains_and_keeps_running() {
    let busy = Hotkey::parse("Shift+PrtScr").unwrap();
    let mut fake = Fake { busy: vec![busy], ..Fake::default() };
    fake.commands.push_back(Command::Capture(Shot::Region));
    let f = go(fake, &["--key", "Shift+PrtScr"]);
    assert_eq!(f.messages.borrow().len(), 1);
    let messages = f.messages.borrow();
    let (text, error) = &messages[0];
    assert!(*error);
    assert!(text.contains("Shift+PrtScr") && text.contains("--key Ctrl+Shift+F12"), "{text}");
    assert!(f.tray.unwrap().0.contains("не работает"));
    // меню трея продолжает работать
    assert_eq!(*f.log.borrow(), ["зырнул: область"]);
}

#[test]
fn commands() {
    let mut fake = Fake::default();
    fake.commands.extend([
        Command::Capture(Shot::Region),
        Command::CaptureLater(Shot::Region, 3),
        Command::About,
        Command::Quit,
        Command::Capture(Shot::Screen), // после «Выхода» уже не обрабатывается
    ]);
    let f = go(fake, &[]);
    assert_eq!(*f.log.borrow(), ["зырнул: область"]);
    assert_eq!(f.scheduled, [(Command::Capture(Shot::Region), 3000)]);
    assert_eq!(f.messages.borrow().len(), 1);
    assert!(f.messages.borrow()[0].0.contains("не утекают"));
    assert_eq!(f.commands.len(), 1);
}

#[test]
fn help_and_errors_in_both_languages() {
    for code in ["ru", "en"] {
        let lang = Lang::from_code(code).unwrap();
        let help = help_text(lang);
        for opt in ["--key", "--key-screen", "--key-window", "--save-dir", "--clear-clipboard", "--no-detect", "--lang", "--help"] {
            assert!(help.contains(opt), "{code}: {opt}");
        }
        assert!(!help.contains('{'), "{code}: неподставленное в справке");
        for bad in [&["--x"][..], &["--key", "A"], &["--key", "Q+F1"], &["--lang", "de"], &["--clear-clipboard", "0"], &["--key", "F1", "--key-screen", "F1"]] {
            let e = match parse(bad.iter().copied()) { Err(e) => e, ok => panic!("{ok:?}") };
            let text = arg_error_text(&e, lang);
            assert!(!text.contains('{'), "{code} {bad:?}: {text}");
            assert!(text.contains("--clear-clipboard"), "к ошибке приложена справка");
        }
    }
}
