//! Что ядро ждёт от ОС. Реализации: `win` (фаза 1–5), `linux`, `mac` (фаза 7).
//!
//! Платформа не принимает решений: она ловит события ОС, превращает их в [`Command`]
//! и выполняет то, что скажет ядро (`app`).

use std::path::{Path, PathBuf};

use crate::hotkey::Hotkey;
use crate::image::Image;
use crate::session::{Cursor, Input, Rect};

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
    /// Снять через `ms` миллисекунд (пункты меню трея, история 6)
    CaptureLater(Shot, u32),
    /// Мышь и клавиатура в режиме выделения
    Overlay(Input),
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

/// Монитор в координатах кадра (физические пиксели).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Monitor {
    pub rect: Rect,
    /// Масштаб, % (100, 125, 150…)
    pub scale: u32,
}

/// Снимок всех мониторов в памяти. Пиксели затираются при удалении (FR-009).
#[derive(Debug)]
pub struct Frame {
    /// Левый верхний угол виртуального экрана в координатах ОС
    pub x: i32,
    pub y: i32,
    pub image: Image,
    pub monitors: Vec<Monitor>,
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

    /// Снять все мониторы в память — сразу, в момент вызова.
    fn capture(&mut self) -> Result<Frame, Problem>;

    /// Показать окно выделения поверх всех мониторов (координаты ОС) и забрать клавиатуру.
    fn show_overlay(&mut self, x: i32, y: i32, width: u32, height: u32) -> Result<(), Problem>;

    /// Вывести готовую картинку в окно выделения, пиксель в пиксель.
    fn present(&mut self, image: &Image);

    fn set_cursor(&mut self, cursor: Cursor);

    fn hide_overlay(&mut self);

    /// Положить картинку в буфер с пометками «не в историю, не в облако» (FR-004).
    /// Буфер обмена программа не читает никогда.
    fn set_clipboard_image(&mut self, image: &Image) -> Result<(), Problem>;

    /// Диалог «Сохранить как» в папке `start`. Фаза 4.
    fn save_dialog(&mut self, start: &Path, file_name: &str) -> Result<Option<PathBuf>, Problem>;

    /// Окно с сообщением; в консоли — текст в неё.
    fn message(&self, title: &str, text: &str, error: bool);

    /// Отладочный вывод (не на диск): OutputDebugString / stderr.
    fn debug_log(&self, text: &str);
}
