//! Зырик: снимки экрана, которые никуда не утекают.
//! Решения принимает ядро (`zyrik::app`), здесь — только запуск и прослойка ОС.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod win;

use zyrik::app;
use zyrik::args::{self, Launch};
use zyrik::i18n::{self, Msg};
#[cfg(any(windows, target_os = "linux"))]
use zyrik::platform::Platform;

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();

    #[cfg(windows)]
    {
        let mut platform = match win::Win::new() {
            Ok(p) => p,
            Err(e) => {
                win::message_box("Zyrik", &e, true);
                std::process::exit(1);
            }
        };
        let system = platform.system_lang();
        let hint = args::lang_hint(&raw);
        let lang = i18n::choose(hint.as_deref(), system.as_deref());
        let title = lang.tr(Msg::AppName);
        match args::parse(&raw) {
            Ok(Launch::Help) => win::explain(title, &app::help_text(lang), false),
            Ok(Launch::Run(opts)) => app::run(&mut platform, &opts, lang),
            Err(e) => {
                win::explain(title, &app::arg_error_text(&e, lang), true);
                std::process::exit(2);
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        let hint = args::lang_hint(&raw);
        // язык нужен и до подключения к X — для --help и ошибок в параметрах
        let env_lang = ["LC_ALL", "LC_MESSAGES", "LANG"].iter().find_map(|v| std::env::var(v).ok().filter(|s| !s.is_empty()));
        let lang = i18n::choose(hint.as_deref(), env_lang.as_deref());
        let title = lang.tr(Msg::AppName);
        match args::parse(&raw) {
            Ok(Launch::Help) => println!("{}", app::help_text(lang)),
            Ok(Launch::Run(opts)) => {
                let mut platform = match linux::Linux::new() {
                    Ok(p) => p,
                    Err(e) => {
                        linux::message(title, &e, true);
                        std::process::exit(1);
                    }
                };
                let lang = i18n::choose(hint.as_deref(), platform.system_lang().as_deref());
                app::run(&mut platform, &opts, lang);
            }
            Err(e) => {
                eprintln!("{}", app::arg_error_text(&e, lang));
                std::process::exit(2);
            }
        }
    }

    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let system = std::env::var("LC_ALL")
            .or_else(|_| std::env::var("LC_MESSAGES"))
            .or_else(|_| std::env::var("LANG"))
            .ok();
        let lang = i18n::choose(args::lang_hint(&raw).as_deref(), system.as_deref());
        match args::parse(&raw) {
            Ok(Launch::Help) => println!("{}", app::help_text(lang)),
            Ok(Launch::Run(_)) => {
                eprintln!("{}", lang.tr(Msg::ErrPlatform));
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("{}", app::arg_error_text(&e, lang));
                std::process::exit(2);
            }
        }
    }
}
