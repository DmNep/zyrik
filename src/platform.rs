//! Что ядро ждёт от ОС. Реализации: `win` (фаза 1–5), `linux`, `mac` (фаза 7).
//!
//! Платформа не принимает решений: она ловит события ОС, превращает их в [`Command`]
//! и выполняет то, что скажет ядро (`app`).

use std::path::{Path, PathBuf};

use crate::hotkey::Hotkey;

/// Что снимаем.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shot {
    Region,
    Screen,
    Window,
}

/// Событие от ОС, уже понятное ядру.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Capture(Shot),
    /// Снять через `secs` секунд (история 6, пункт меню трея)
    CaptureLater(Shot, u32),
    About,
    Quit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// Уже запущена другая копия (FR-015)
    AlreadyRunning,
    /// Клавишу держит другая программа
    KeyBusy,
    /// Не удалось по другой причине — текст ОС
    Os(String),
    /// Возможность появится в следующих фазах
    NotYet(&'static str),
}

/// Кадр экрана в памяти, RGBA. Затирается после использования (FR-009).
pub struct Frame {
    /// Левый верхний угол виртуального экрана
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Пункт меню трея.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    pub text: String,
    pub command: Command,
}

pub trait Platform {
    /// Код языка системы (`ru-RU`), если ОС его сообщает.
    fn system_lang(&self) -> Option<String>;

    /// Занять «место» единственной копии; если занято — [`Problem::AlreadyRunning`].
    fn claim_single_instance(&mut self) -> Result<(), Problem>;

    /// Показать значок в трее с подсказкой и меню.
    fn show_tray(&mut self, tip: &str, menu: &[MenuItem]) -> Result<(), Problem>;

    /// Сменить подсказку значка.
    fn set_tray_tip(&mut self, tip: &str);

    /// Глобальная горячая клавиша → `Command::Capture(shot)`.
    fn register_hotkey(&mut self, shot: Shot, key: Hotkey) -> Result<(), Problem>;

    /// Через `delay_ms` выдать `command` из [`Platform::next_command`].
    fn schedule(&mut self, command: Command, delay_ms: u32);

    /// Ждать следующую команду. `None` — ОС просит завершиться.
    fn next_command(&mut self) -> Option<Command>;

    /// Снять все мониторы в память. Фаза 2.
    fn capture(&mut self) -> Result<Frame, Problem>;

    /// Показать оверлей выделения поверх кадра. Фаза 2.
    fn show_overlay(&mut self, frame: &Frame) -> Result<(), Problem>;

    /// Положить картинку в буфер с пометками «не в историю, не в облако» (FR-004). Фаза 2.
    fn set_clipboard_image(&mut self, width: u32, height: u32, rgba: &[u8]) -> Result<(), Problem>;

    /// Диалог «Сохранить как» в папке `start`. Фаза 4.
    fn save_dialog(&mut self, start: &Path, file_name: &str) -> Result<Option<PathBuf>, Problem>;

    /// Окно с сообщением; в консоли — текст в неё.
    fn message(&self, title: &str, text: &str, error: bool);

    /// Отладочный вывод (не на диск): OutputDebugString / stderr.
    fn debug_log(&self, text: &str);
}
