//! Главный цикл с поддельной платформой: что ядро просит у ОС в ответ на команды.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use zyrik::app::{arg_error_text, help_text, run};
use zyrik::export::Stamp;
use zyrik::args::{parse, Launch, Options};
use zyrik::hotkey::Hotkey;
use zyrik::i18n::Lang;
use zyrik::image::Image;
use zyrik::platform::{Command, Frame, MenuItem, Monitor, Platform, Problem, Shot};
use zyrik::session::{Button, Cursor, Input, Key, Pt, Rect};

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
    captures: u32,
    overlay: Option<(i32, i32, u32, u32)>,
    presented: u32,
    cursor: Option<Cursor>,
    /// что легло в буфер: размер и пиксели
    clipboard: Option<(u32, u32, Vec<u32>)>,
    pictures: Option<PathBuf>,
    /// какие диалоги открывались: стартовая папка, имя
    dialogs: Vec<(Option<PathBuf>, String)>,
    dialog_answer: Option<PathBuf>,
}

/// Кадр 400×300 в координатах ОС (-100, 0): у каждого пикселя своё значение.
fn test_frame() -> Frame {
    let (w, h) = (400u32, 300u32);
    let px = (0..w * h).map(|i| 0xFF00_0000 | i).collect();
    Frame {
        x: -100,
        y: 0,
        image: Image::from_pixels(w, h, px),
        monitors: vec![Monitor { rect: Rect { x: 0, y: 0, w: 400, h: 300 }, scale: 100 }],
    }
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
        assert!(self.overlay.is_none(), "снимать, пока на экране наше окно, нельзя");
        self.captures += 1;
        Ok(test_frame())
    }
    fn show_overlay(&mut self, x: i32, y: i32, w: u32, h: u32) -> Result<(), Problem> {
        self.overlay = Some((x, y, w, h));
        Ok(())
    }
    fn present(&mut self, image: &Image) {
        assert!(self.overlay.is_some());
        assert_eq!((image.width(), image.height()), (400, 300));
        self.presented += 1;
    }
    fn set_cursor(&mut self, cursor: Cursor) {
        self.cursor = Some(cursor);
    }
    fn hide_overlay(&mut self) {
        self.overlay = None;
    }
    fn set_clipboard_image(&mut self, image: Image) -> Result<(), Problem> {
        assert!(self.overlay.is_none(), "буфер — уже после того, как окно спрятано");
        self.clipboard = Some((image.width(), image.height(), image.pixels().to_vec()));
        Ok(())
    }
    fn save_dialog(&mut self, start: Option<&Path>, name: &str) -> Result<Option<PathBuf>, Problem> {
        self.dialogs.push((start.map(Path::to_path_buf), name.to_string()));
        Ok(self.dialog_answer.clone())
    }
    fn pictures_dir(&self) -> Option<PathBuf> {
        self.pictures.clone()
    }
    fn local_time(&self) -> Stamp {
        Stamp { year: 2026, month: 11, day: 21, hour: 9, minute: 5, second: 7 }
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
    fake.commands.push_back(Command::CaptureLater(Shot::Region, 250));
    let f = go(fake, &["--key", "Shift+PrtScr"]);
    {
        let messages = f.messages.borrow();
        assert_eq!(messages.len(), 1);
        let (text, error) = &messages[0];
        assert!(*error);
        assert!(text.contains("Shift+PrtScr") && text.contains("--key Ctrl+Shift+F12"), "{text}");
    }
    assert!(f.tray.as_ref().unwrap().0.contains("не работает"));
    // меню трея продолжает работать
    assert_eq!(f.scheduled, [(Command::Capture(Shot::Region), 250)]);
}

#[test]
fn tray_menu_commands() {
    let mut fake = Fake::default();
    fake.commands.extend([
        Command::CaptureLater(Shot::Region, 3000),
        Command::About,
        Command::Quit,
        Command::Capture(Shot::Screen), // после «Выхода» уже не обрабатывается
    ]);
    let f = go(fake, &[]);
    assert_eq!(f.scheduled, [(Command::Capture(Shot::Region), 3000)]);
    assert_eq!(f.messages.borrow().len(), 1);
    assert!(f.messages.borrow()[0].0.contains("не утекают"));
    assert_eq!(f.commands.len(), 1);
    assert_eq!(f.captures, 0);
}

fn ov(i: Input) -> Command {
    Command::Overlay(i)
}
fn left_down(x: i32, y: i32) -> Command {
    ov(Input::Down(Button::Left, Pt { x, y }))
}
fn left_up(x: i32, y: i32) -> Command {
    ov(Input::Up(Button::Left, Pt { x, y }))
}
fn key(k: Key) -> Command {
    ov(Input::Key { key: k, shift: false, ctrl: false })
}

#[test]
fn hotkey_opens_overlay_over_whole_virtual_screen() {
    let mut fake = Fake::default();
    fake.commands.push_back(Command::Capture(Shot::Region));
    let f = go(fake, &[]);
    assert_eq!(f.captures, 1);
    // после выхода из цикла окно спрятано
    assert_eq!(f.overlay, None);
    assert_eq!(f.presented, 1);
    assert_eq!(f.cursor, Some(Cursor::Cross));
    assert!(f.log.borrow().iter().any(|l| l.contains("SC-001")));
}

#[test]
fn select_and_enter_puts_exact_pixels_on_clipboard() {
    let mut fake = Fake::default();
    fake.commands.extend([
        Command::Capture(Shot::Region),
        left_down(10, 20),
        ov(Input::Move(Pt { x: 60, y: 50 })),
        left_up(110, 70),
        key(Key::Enter),
    ]);
    let f = go(fake, &[]);
    let (w, h, px) = f.clipboard.expect("снимок в буфере");
    assert_eq!((w, h), (100, 50));
    let frame = test_frame();
    let want = frame.image.crop(Rect { x: 10, y: 20, w: 100, h: 50 });
    assert_eq!(px, want.pixels());
    assert_eq!(f.overlay, None);
    assert!(f.log.borrow().iter().any(|l| l.contains("зырнул: область 100x50")));
    assert!(f.messages.borrow().is_empty());
}

#[test]
fn esc_closes_without_touching_clipboard() {
    let mut fake = Fake::default();
    fake.commands.extend([
        Command::Capture(Shot::Region),
        left_down(10, 20),
        left_up(110, 70),
        key(Key::Esc),
    ]);
    let f = go(fake, &[]);
    assert!(f.clipboard.is_none());
    assert_eq!(f.overlay, None);
}

#[test]
fn hotkey_while_selecting_is_ignored_then_works_again() {
    let mut fake = Fake::default();
    fake.commands.extend([
        Command::Capture(Shot::Region),
        Command::Capture(Shot::Region),
        Command::CaptureLater(Shot::Region, 3000),
        key(Key::Esc),
        Command::Capture(Shot::Region),
    ]);
    let f = go(fake, &[]);
    assert_eq!(f.captures, 2, "второе нажатие при открытом выделении не снимает");
    assert!(f.scheduled.is_empty());
}

#[test]
fn input_without_overlay_is_ignored() {
    let mut fake = Fake::default();
    fake.commands.extend([left_down(1, 1), key(Key::Enter)]);
    let f = go(fake, &[]);
    assert!(f.clipboard.is_none());
    assert_eq!(f.presented, 0);
}

#[test]
fn cursor_follows_selection() {
    let mut fake = Fake::default();
    fake.commands.extend([
        Command::Capture(Shot::Region),
        left_down(10, 20),
        left_up(110, 70),
        ov(Input::Move(Pt { x: 50, y: 40 })),
    ]);
    let f = go(fake, &[]);
    assert_eq!(f.cursor, Some(Cursor::Move));
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

// ---------- фаза 3: пометки попадают в снимок, сохранение ----------

fn ctrl(k: char) -> Command {
    ov(Input::Key { key: Key::Letter(k), shift: false, ctrl: true })
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("zyrik-app-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn select_and_draw() -> Vec<Command> {
    vec![
        Command::Capture(Shot::Region),
        left_down(10, 20),
        left_up(110, 70),
        ov(Input::Key { key: Key::Letter('R'), shift: false, ctrl: false }),
        left_down(30, 30),
        ov(Input::Move(Pt { x: 60, y: 50 })),
        left_up(80, 60),
    ]
}

#[test]
fn marks_are_baked_into_the_clipboard_shot() {
    let mut fake = Fake::default();
    fake.commands.extend(select_and_draw());
    fake.commands.push_back(key(Key::Enter));
    let f = go(fake, &[]);
    let (w, h, px) = f.clipboard.unwrap();
    assert_eq!((w, h), (100, 50));
    let plain = test_frame().image.crop(Rect { x: 10, y: 20, w: 100, h: 50 });
    assert_ne!(px, plain.pixels(), "прямоугольник нарисован в снимке");
    // угол прямоугольника (30,30) кадра → (20,10) снимка — красный
    let p = px[10 * 100 + 20];
    assert!((p >> 16) & 0xFF > 180, "{p:#x}");
}

#[test]
fn ctrl_s_saves_png_to_pictures_without_dialog() {
    let dir = temp_dir("ctrl-s");
    let mut fake = Fake { pictures: Some(dir.clone()), ..Fake::default() };
    fake.commands.extend(select_and_draw());
    fake.commands.push_back(ctrl('S'));
    let f = go(fake, &[]);
    assert!(f.dialogs.is_empty(), "без диалога");
    assert!(f.clipboard.is_none(), "Ctrl+S — только файл");
    assert_eq!(f.overlay, None, "режим закрылся");
    let file = dir.join("Зырик_2026-11-21_09-05-07.png");
    let bytes = std::fs::read(&file).expect("файл сохранён");
    assert_eq!(&bytes[..4], b"\x89PNG");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn save_dir_option_wins_over_pictures() {
    let dir = temp_dir("save-dir");
    let other = temp_dir("pictures");
    let mut fake = Fake { pictures: Some(other.clone()), ..Fake::default() };
    fake.commands.extend([Command::Capture(Shot::Region), left_down(10, 20), left_up(110, 70), ctrl('S')]);
    let f = go(fake, &["--save-dir", dir.to_str().unwrap()]);
    assert!(f.dialogs.is_empty());
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    assert_eq!(std::fs::read_dir(&other).unwrap().count(), 0);
    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&other).unwrap();
}

#[test]
fn missing_folder_opens_dialog_and_cancel_keeps_selection() {
    let gone = std::env::temp_dir().join("zyrik-нет-такой-папки-123");
    let mut fake = Fake { dialog_answer: None, ..Fake::default() };
    fake.commands.extend([Command::Capture(Shot::Region), left_down(10, 20), left_up(110, 70), ctrl('S'), key(Key::Enter)]);
    let f = go(fake, &["--save-dir", gone.to_str().unwrap()]);
    assert_eq!(f.dialogs.len(), 1, "недоступная папка → диалог");
    assert_eq!(f.dialogs[0].1, "Зырик_2026-11-21_09-05-07.png");
    assert!(!gone.exists(), "папку сами не создали");
    // отменил диалог — выделение осталось, Enter положил снимок в буфер
    assert!(f.clipboard.is_some());
}

#[test]
fn floppy_dialog_choice_becomes_last_folder() {
    let first = temp_dir("dialog-a");
    let chosen = temp_dir("dialog-b");
    let mut fake = Fake {
        pictures: Some(first.clone()),
        dialog_answer: Some(chosen.join("мой снимок.png")),
        ..Fake::default()
    };
    // щелчок по дискете на панели под выделенной зоной
    let sel = Rect { x: 10, y: 20, w: 100, h: 50 };
    let bar = zyrik::toolbar::layout(sel, Rect { x: 0, y: 0, w: 400, h: 300 }, &test_frame().monitors);
    let fl = bar.button_rect(zyrik::toolbar::Button::Save).unwrap();
    fake.commands.extend([
        Command::Capture(Shot::Region),
        left_down(10, 20),
        left_up(110, 70),
        left_down(fl.x + 4, fl.y + 4),
    ]);
    // второй снимок — Ctrl+S уже без диалога, в выбранную папку
    fake.commands.extend([Command::Capture(Shot::Region), left_down(10, 20), left_up(110, 70), ctrl('S')]);
    let f = go(fake, &[]);
    assert_eq!(f.dialogs.len(), 1);
    assert_eq!(f.dialogs[0].0.as_deref(), Some(first.as_path()), "диалог открылся в последней папке");
    assert!(chosen.join("мой снимок.png").exists());
    assert!(chosen.join("Зырик_2026-11-21_09-05-07.png").exists(), "Ctrl+S — в папку из диалога");
    assert_eq!(std::fs::read_dir(&first).unwrap().count(), 0);
    std::fs::remove_dir_all(&first).unwrap();
    std::fs::remove_dir_all(&chosen).unwrap();
}
