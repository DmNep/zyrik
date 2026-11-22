//! Главный цикл: общий для всех ОС, проверяется тестами с поддельной платформой.

use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::annot::render;
use crate::export;
use crate::args::{program_path, ArgError, Options, MAX_CLEAR_MINUTES};
use crate::hotkey::{Hotkey, KeyError, META_NAME};
use crate::i18n::{Lang, Msg};
use crate::image::Image;
use crate::picker::{self, PickOutcome, Picker, Status};
use crate::platform::{Command, Frame, MenuItem, PlaceKind, Platform, Problem, Shot};
use crate::session::{Cursor, Input, Outcome, Session};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Задержка пункта «Зырнуть через 3 с»
pub const LATER_MS: u32 = 3000;
/// Пункт меню «Зырнуть» снимает не сразу: даём меню трея погаснуть, иначе оно попадёт в кадр.
pub const MENU_FADE_MS: u32 = 250;

pub fn help_text(lang: Lang) -> String {
    lang.fmt(
        Msg::HelpText,
        &[
            ("version", VERSION),
            ("default_key", &Hotkey::default_capture().to_string()),
            ("meta", META_NAME),
            ("langs", &Lang::codes()),
            ("program", &program_path()),
        ],
    )
}

/// Ошибка параметров + справка: человек сразу видит, как надо.
pub fn arg_error_text(err: &ArgError, lang: Lang) -> String {
    let error = match err {
        ArgError::Unknown(arg) => lang.fmt(Msg::ErrUnknown, &[("arg", arg)]),
        ArgError::MissingValue(o) => lang.fmt(Msg::ErrMissingValue, &[("option", o)]),
        ArgError::Duplicate(o) => lang.fmt(Msg::ErrDuplicate, &[("option", o)]),
        ArgError::BadKey { option, value, reason } => {
            let reason = match reason {
                KeyError::Empty => lang.tr(Msg::KeyEmpty).to_string(),
                KeyError::Unknown(p) => lang.fmt(Msg::KeyUnknown, &[("part", p)]),
                KeyError::NoKey => lang.tr(Msg::KeyNoKey).to_string(),
                KeyError::TwoKeys => lang.tr(Msg::KeyTwoKeys).to_string(),
                KeyError::Repeated(p) => lang.fmt(Msg::KeyRepeated, &[("part", p)]),
                KeyError::NeedsModifier => lang.fmt(Msg::KeyNeedsModifier, &[("meta", META_NAME)]),
            };
            lang.fmt(Msg::ErrBadKey, &[("option", option), ("value", value), ("reason", &reason)])
        }
        ArgError::BadMinutes(v) => {
            lang.fmt(Msg::ErrBadMinutes, &[("value", v), ("max", &MAX_CLEAR_MINUTES.to_string())])
        }
        ArgError::BadLang(v) => lang.fmt(Msg::ErrBadLang, &[("value", v), ("langs", &Lang::codes())]),
        ArgError::SameKey(a, b) => lang.fmt(Msg::ErrSameKey, &[("a", a), ("b", b)]),
    };
    format!("{}\n\n{}", lang.fmt(Msg::ErrWrap, &[("error", &error)]), help_text(lang))
}

fn shot_name(lang: Lang, shot: Shot) -> &'static str {
    lang.tr(match shot {
        Shot::Region => Msg::ShotRegion,
        Shot::Screen => Msg::ShotScreen,
        Shot::Window => Msg::ShotWindow,
    })
}

/// Работает, пока не выбран «Выход» или ОС не попросит завершиться.
pub fn run<P: Platform>(p: &mut P, opts: &Options, lang: Lang) {
    let title = lang.tr(Msg::AppName);
    match p.claim_single_instance() {
        Ok(()) => {}
        Err(Problem::AlreadyRunning) => {
            p.message(title, lang.tr(Msg::InstanceRunning), false);
            return;
        }
        Err(e) => p.debug_log(&format!("single instance: {e:?}")),
    }

    let menu = [
        MenuItem {
            text: lang.tr(Msg::TrayCapture).into(),
            command: Command::CaptureLater(Shot::Region, MENU_FADE_MS),
        },
        MenuItem {
            text: lang.tr(Msg::TrayCaptureLater).into(),
            command: Command::CaptureLater(Shot::Region, LATER_MS),
        },
        MenuItem { text: lang.tr(Msg::TrayAbout).into(), command: Command::About },
        MenuItem { text: lang.tr(Msg::TrayExit).into(), command: Command::Quit },
    ];
    let key = opts.key.to_string();
    let tip_ok = lang.fmt(Msg::TrayTip, &[("version", VERSION), ("key", &key)]);
    if let Err(e) = p.show_tray(&tip_ok, &menu) {
        p.message(title, &format!("{e:?}"), true);
        return;
    }

    let program = program_path();
    let keys = [
        ("--key", Shot::Region, Some(opts.key)),
        ("--key-screen", Shot::Screen, opts.key_screen),
        ("--key-window", Shot::Window, opts.key_window),
    ];
    for (option, shot, hk) in keys {
        let Some(hk) = hk else { continue };
        let k = hk.to_string();
        let text = match p.register_hotkey(shot, hk) {
            Ok(()) => continue,
            Err(Problem::KeyBusy) => {
                lang.fmt(Msg::HotkeyBusy, &[("key", &k), ("option", option), ("program", &program)])
            }
            Err(e) => lang.fmt(
                Msg::HotkeyFailed,
                &[("key", &k), ("option", option), ("error", &format!("{e:?}"))],
            ),
        };
        if shot == Shot::Region {
            p.set_tray_tip(&lang.fmt(Msg::TrayTipNoKey, &[("version", VERSION)]));
        }
        p.message(title, &text, true);
    }

    // «Последняя папка» для Ctrl+S — только в памяти (история 3).
    let mut last_dir: Option<PathBuf> = opts.save_dir.clone().or_else(|| {
        p.places().into_iter().find(|pl| pl.kind == PlaceKind::Pictures).map(|pl| pl.path)
    });
    // Открытый режим выделения. При удалении кадр и холст затираются (Image::drop).
    let mut active: Option<Active> = None;
    while let Some(cmd) = p.next_command() {
        match cmd {
            Command::Capture(shot) => {
                // T028: повторное нажатие при открытом выделении ничего не делает
                if active.is_none() {
                    active = start(p, lang, shot, opts.detect);
                }
            }
            Command::CaptureLater(shot, ms) => {
                if active.is_none() {
                    p.schedule(Command::Capture(shot), ms);
                }
            }
            Command::Overlay(input) => {
                let Some(a) = active.as_mut() else { continue };
                if a.picker.is_some() {
                    // открыт выбор папки — весь ввод ему
                    let Some(dir) = a.pick(p, input) else { continue };
                    match a.save_into(p, &dir) {
                        Ok(path) => {
                            saved(p, lang, &path);
                            last_dir = Some(dir);
                            p.hide_overlay();
                            active = None;
                        }
                        Err(e) => {
                            if let Some(pk) = a.picker.as_mut() {
                                pk.set_status(Status::SaveFailed(e));
                            }
                            a.redraw(p);
                        }
                    }
                    continue;
                }
                let outcome = a.session.input(input);
                if let Input::Down(_, pt) | Input::Move(pt) | Input::Up(_, pt) = input {
                    p.set_cursor(a.session.cursor_at(pt));
                }
                match outcome {
                    Outcome::Nothing => {}
                    Outcome::Redraw => a.redraw(p),
                    Outcome::Cancel => {
                        p.hide_overlay();
                        active = None;
                    }
                    Outcome::Confirm(rect) => {
                        let shot = a.bake(rect);
                        let what = shot_name(lang, a.shot);
                        let (sw, sh) = (shot.width(), shot.height());
                        // обычно буфер — после того, как окно спрятано; на Wayland без data-control —
                        // пока окно ещё в фокусе, иначе композитор буфер не даст
                        let result = if p.clipboard_needs_focus() {
                            let r = p.set_clipboard_image(shot);
                            p.hide_overlay();
                            r
                        } else {
                            p.hide_overlay();
                            p.set_clipboard_image(shot)
                        };
                        active = None;
                        match result {
                            Ok(()) => p.debug_log(&format!(
                                "{} {sw}x{sh}",
                                lang.fmt(Msg::LogCaptured, &[("shot", what)])
                            )),
                            Err(e) => p.message(
                                title,
                                &lang.fmt(Msg::ClipboardFailed, &[("error", &format!("{e:?}"))]),
                                true,
                            ),
                        }
                        // снимок теперь у платформы: она затрёт его после вставки
                    }
                    Outcome::Save(_) => {
                        // Ctrl+S — сразу в последнюю папку; не вышло — выбор папки с объяснением,
                        // снимок не теряется
                        let status = match last_dir.clone() {
                            Some(dir) => match a.save_into(p, &dir) {
                                Ok(path) => {
                                    saved(p, lang, &path);
                                    p.hide_overlay();
                                    active = None;
                                    continue;
                                }
                                Err(e) => Some(Status::Unavailable(dir, e)),
                            },
                            None => None,
                        };
                        a.open_picker(p, last_dir.as_deref(), status);
                    }
                    Outcome::SaveAs(_) => a.open_picker(p, last_dir.as_deref(), None),
                }
            }
            Command::Tick => {
                let Some(a) = active.as_mut() else { continue };
                let Some(rx) = &a.ocr else { continue };
                match rx.try_recv() {
                    Ok(found) => {
                        a.ocr = None;
                        a.session.set_findings(found);
                        a.redraw(p);
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => p.schedule(Command::Tick, TICK_MS),
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        a.ocr = None;
                        a.session.set_findings(Vec::new());
                        a.redraw(p);
                    }
                }
            }
            Command::About => {
                let text = lang.fmt(
                    Msg::AboutText,
                    &[("version", VERSION), ("key", &key), ("program", &program)],
                );
                p.message(title, &text, false);
            }
            Command::Quit => break,
        }
    }
    if active.is_some() {
        p.hide_overlay();
    }
}

/// Как часто проверять, готово ли распознавание
const TICK_MS: u32 = 100;

struct Active {
    /// Находки из потока распознавания (пока он работает)
    ocr: Option<std::sync::mpsc::Receiver<Vec<crate::redact::Finding>>>,
    shot: Shot,
    lang: Lang,
    frame: Frame,
    session: Session,
    /// Сюда собирается картинка оверлея; размер как у кадра
    canvas: Image,
    /// Открыт выбор папки (дискета или Ctrl+S, который не смог сохранить)
    picker: Option<Picker>,
}

impl Active {
    fn redraw<P: Platform>(&mut self, p: &mut P) {
        render::overlay(&self.frame.image, &self.session, &mut self.canvas, self.lang);
        if let Some(pk) = &self.picker {
            render::picker(&mut self.canvas, pk, self.lang);
        }
        p.present(&self.canvas);
    }

    /// Зона с пометками — то, что уходит в буфер или файл.
    fn bake(&self, rect: crate::session::Rect) -> Image {
        render::bake(&self.frame.image, rect, self.session.marks().items(), self.session.live().as_ref(), &self.session.redactions())
    }

    /// Открыть выбор папки на мониторе зоны, начиная с `start`.
    fn open_picker<P: Platform>(&mut self, p: &mut P, start: Option<&Path>, status: Option<Status>) {
        let Some(sel) = self.session.selection() else { return };
        let (w, h) = (self.frame.image.width() as i32, self.frame.image.height() as i32);
        let screen = crate::session::Rect { x: 0, y: 0, w, h };
        let monitor = picker::monitor_for(sel, screen, &self.frame.monitors);
        let name = export::file_name(self.lang.tr(Msg::AppName), p.local_time());
        let mut pk = Picker::open(start, p.places(), name, monitor);
        if let Some(s) = status {
            pk.set_status(s);
        }
        self.picker = Some(pk);
        p.set_cursor(Cursor::Arrow);
        self.redraw(p);
    }

    /// Ввод в выбор папки. `Some(папка)` — человек выбрал, куда сохранить.
    fn pick<P: Platform>(&mut self, p: &mut P, input: Input) -> Option<PathBuf> {
        let pk = self.picker.as_mut()?;
        if let Input::Move(_) | Input::Down(..) = input {
            p.set_cursor(Cursor::Arrow);
        }
        match pk.input(input) {
            PickOutcome::Nothing => None,
            PickOutcome::Redraw => {
                self.redraw(p);
                None
            }
            PickOutcome::Close => {
                self.picker = None;
                self.redraw(p);
                None
            }
            PickOutcome::Save(dir) => Some(dir),
        }
    }

    /// Записать снимок в `dir` под новым именем, ничего не перезаписывая (история 3).
    fn save_into<P: Platform>(&self, p: &P, dir: &Path) -> Result<PathBuf, String> {
        let sel = self.session.selection().ok_or_else(String::new)?;
        let png = export::encode_png(&self.bake(sel));
        let name = export::file_name(self.lang.tr(Msg::AppName), p.local_time());
        export::save_unique(dir, &name, &png).map_err(|e| {
            p.debug_log(&format!("{}: {e}", dir.display()));
            e.to_string()
        })
    }
}

fn saved<P: Platform>(p: &P, lang: Lang, path: &Path) {
    p.debug_log(&lang.fmt(Msg::LogSaved, &[("path", &path.display().to_string())]));
}

/// Снять экран и открыть выделение. Захват — первым делом, пока на экране ничего нашего.
fn start<P: Platform>(p: &mut P, lang: Lang, shot: Shot, detect: bool) -> Option<Active> {
    let t0 = Instant::now();
    let frame = match p.capture() {
        Ok(f) => f,
        Err(Problem::NoPermission) => {
            p.message(lang.tr(Msg::AppName), lang.tr(Msg::CapturePermission), true);
            return None;
        }
        Err(e) => {
            let text = lang.fmt(Msg::CaptureFailed, &[("error", &format!("{e:?}"))]);
            p.message(lang.tr(Msg::AppName), &text, true);
            return None;
        }
    };
    let captured = t0.elapsed();
    // Экран и окно (история 5) — фаза 5; пока любая клавиша открывает выделение области.
    let (w, h) = (frame.image.width(), frame.image.height());
    let mut session = Session::with_monitors(w, h, frame.monitors.clone());
    // Поиск опасного: распознавание всего кадра в отдельном потоке — выделение не ждёт.
    // Копия кадра у потока затирается при удалении (Image::drop).
    let ocr = (detect && crate::ocr::available()).then(|| {
        let copy = Image::from_pixels(w, h, frame.image.pixels().to_vec());
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let words = crate::ocr::recognize(&copy);
            let _ = tx.send(crate::redact::detect(&words));
        });
        session.start_detect();
        p.schedule(Command::Tick, TICK_MS);
        rx
    });
    let mut a = Active { ocr, shot, lang, session, canvas: Image::new(w, h), frame, picker: None };
    if let Err(e) = p.show_overlay(a.frame.x, a.frame.y, w, h) {
        p.message(lang.tr(Msg::AppName), &format!("{e:?}"), true);
        return None;
    }
    p.set_cursor(Cursor::Cross);
    a.redraw(p);
    p.debug_log(&format!(
        "{w}x{h}: снято за {} мс, выделение на экране через {} мс (SC-001: ≤ 200)",
        captured.as_millis(),
        t0.elapsed().as_millis()
    ));
    Some(a)
}
