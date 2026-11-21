//! Главный цикл: общий для всех ОС, проверяется тестами с поддельной платформой.

use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::annot::render;
use crate::export;
use crate::args::{program_path, ArgError, Options, MAX_CLEAR_MINUTES};
use crate::hotkey::{Hotkey, KeyError, META_NAME};
use crate::i18n::{Lang, Msg};
use crate::image::Image;
use crate::platform::{Command, Frame, MenuItem, Platform, Problem, Shot};
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
    let mut last_dir: Option<PathBuf> = opts.save_dir.clone().or_else(|| p.pictures_dir());
    // Открытый режим выделения. При удалении кадр и холст затираются (Image::drop).
    let mut active: Option<Active> = None;
    while let Some(cmd) = p.next_command() {
        match cmd {
            Command::Capture(shot) => {
                // T028: повторное нажатие при открытом выделении ничего не делает
                if active.is_none() {
                    active = start(p, lang, shot);
                }
            }
            Command::CaptureLater(shot, ms) => {
                if active.is_none() {
                    p.schedule(Command::Capture(shot), ms);
                }
            }
            Command::Overlay(input) => {
                let Some(a) = active.as_mut() else { continue };
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
                        p.hide_overlay();
                        active = None;
                        let (sw, sh) = (shot.width(), shot.height());
                        match p.set_clipboard_image(shot) {
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
                    Outcome::Save(rect) | Outcome::SaveAs(rect) => {
                        let dialog = matches!(outcome, Outcome::SaveAs(_));
                        let shot = a.bake(rect);
                        if let Some(path) = save(p, lang, &shot, last_dir.as_deref(), dialog) {
                            p.debug_log(&lang.fmt(Msg::LogSaved, &[("path", &path.display().to_string())]));
                            last_dir = path.parent().map(Path::to_path_buf);
                            p.hide_overlay();
                            active = None;
                        } else if let Some(a) = active.as_mut() {
                            // передумал или не вышло — снимок не теряется, выделение остаётся
                            a.redraw(p);
                        }
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

struct Active {
    shot: Shot,
    lang: Lang,
    frame: Frame,
    session: Session,
    /// Сюда собирается картинка оверлея; размер как у кадра
    canvas: Image,
}

impl Active {
    fn redraw<P: Platform>(&mut self, p: &mut P) {
        render::overlay(&self.frame.image, &self.session, &mut self.canvas, self.lang);
        p.present(&self.canvas);
    }

    /// Зона с пометками — то, что уходит в буфер или файл.
    fn bake(&self, rect: crate::session::Rect) -> Image {
        render::bake(&self.frame.image, rect, self.session.marks().items(), self.session.live().as_ref())
    }
}

/// Ctrl+S — сразу в последнюю папку; дискета, недоступная папка или её отсутствие — диалог.
/// Возвращает путь сохранённого файла; `None` — не сохранили (передумал или ошибка, о ней сказано).
fn save<P: Platform>(p: &mut P, lang: Lang, shot: &Image, last_dir: Option<&Path>, dialog: bool) -> Option<PathBuf> {
    let title = lang.tr(Msg::AppName);
    let png = export::encode_png(shot);
    let name = export::file_name(title, p.local_time());
    if !dialog {
        if let Some(dir) = last_dir {
            match export::save_unique(dir, &name, &png) {
                Ok(path) => return Some(path),
                Err(e) => p.debug_log(&format!("{}: {e} — открываю диалог", dir.display())),
            }
        }
    }
    let path = match p.save_dialog(last_dir, &name) {
        Ok(Some(path)) => path,
        Ok(None) => return None,
        Err(e) => {
            p.message(title, &lang.fmt(Msg::SaveFailed, &[("error", &format!("{e:?}"))]), true);
            return None;
        }
    };
    match export::save_to(&path, &png) {
        Ok(()) => Some(path),
        Err(e) => {
            let error = format!("{}: {e}", path.display());
            p.message(title, &lang.fmt(Msg::SaveFailed, &[("error", &error)]), true);
            None
        }
    }
}

/// Снять экран и открыть выделение. Захват — первым делом, пока на экране ничего нашего.
fn start<P: Platform>(p: &mut P, lang: Lang, shot: Shot) -> Option<Active> {
    let t0 = Instant::now();
    let frame = match p.capture() {
        Ok(f) => f,
        Err(e) => {
            let text = lang.fmt(Msg::CaptureFailed, &[("error", &format!("{e:?}"))]);
            p.message(lang.tr(Msg::AppName), &text, true);
            return None;
        }
    };
    let captured = t0.elapsed();
    // Экран и окно (история 5) — фаза 5; пока любая клавиша открывает выделение области.
    let (w, h) = (frame.image.width(), frame.image.height());
    let session = Session::with_monitors(w, h, frame.monitors.clone());
    let mut a = Active { shot, lang, session, canvas: Image::new(w, h), frame };
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
