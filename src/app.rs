//! Главный цикл: общий для всех ОС, проверяется тестами с поддельной платформой.

use crate::args::{program_path, ArgError, Options, MAX_CLEAR_MINUTES};
use crate::hotkey::{KeyError, META_NAME, Hotkey};
use crate::i18n::{Lang, Msg};
use crate::platform::{Command, MenuItem, Platform, Problem, Shot};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Задержка пункта «Зырнуть через 3 с»
pub const LATER_SECS: u32 = 3;

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
        MenuItem { text: lang.tr(Msg::TrayCapture).into(), command: Command::Capture(Shot::Region) },
        MenuItem {
            text: lang.tr(Msg::TrayCaptureLater).into(),
            command: Command::CaptureLater(Shot::Region, LATER_SECS),
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

    while let Some(cmd) = p.next_command() {
        match cmd {
            Command::Capture(shot) => {
                // Фаза 2: захват → оверлей. Пока — отметка, что клавиша дошла.
                p.debug_log(&lang.fmt(Msg::LogCaptured, &[("shot", shot_name(lang, shot))]));
            }
            Command::CaptureLater(shot, secs) => p.schedule(Command::Capture(shot), secs * 1000),
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
}
