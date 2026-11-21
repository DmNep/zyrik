//! Ядро Зырика: всё, что не зависит от ОС. Прослойки ОС — в `src/win` и далее.
//!
//! Принцип I конституции: ни сети, ни записи на диск (кроме Ctrl+S).

pub mod app;
pub mod args;
pub mod hotkey;
pub mod i18n;
pub mod platform;
