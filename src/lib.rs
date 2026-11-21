//! Ядро Зырика: всё, что не зависит от ОС. Прослойки ОС — в `src/win` и далее.
//!
//! Принцип I конституции: ни сети, ни записи на диск (кроме Ctrl+S).

pub mod annot;
pub mod app;
pub mod args;
pub mod hotkey;
pub mod i18n;
pub mod image;
pub mod oneshot;
pub mod platform;
pub mod session;
