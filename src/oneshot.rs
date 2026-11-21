//! Одноразовый снимок в буфере обмена: вставили — и он исчез (FR-004a).
//!
//! Платформа кладёт в буфер не картинку, а обещание (отложенная выдача). Когда какая-то
//! программа просит данные, платформа спрашивает здесь, отдавать ли их и что делать дальше.
//!
//! - **Зажат Ctrl или Win** — это Ctrl+V / Win+V. Отдаём и ждём, пока модификатор отпустят:
//!   пока держат, можно вставить ещё раз. Отпустили — буфер очищается.
//! - **Просит программа на переднем плане, без модификатора** — «Вставить» из меню или
//!   Shift+Insert. Отдаём и снова кладём обещание, чтобы увидеть следующую вставку.
//! - **Просит фоновая программа** (синхронизаторы и менеджеры буфера, например msrdc из WSLg,
//!   который не смотрит на пометку «исключить из мониторинга») — **не отдаём**. Обещание
//!   остаётся в буфере, настоящая вставка его получит.
//!
//! Перевыдач без модификатора не больше [`MAX_REARMS`]: программа на переднем плане,
//! читающая каждое изменение буфера, не должна загнать нас в бесконечный круг.

pub const MAX_REARMS: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// Не отдавать: фоновый читатель
    Refuse,
    /// Отдать, следить за модификатором и очистить буфер, когда отпустят
    GiveAndWatch,
    /// Отдать и снова положить обещание
    GiveAndRearm,
    /// Отдать и больше не следить (перевыдачи кончились)
    Give,
}

#[derive(Debug, Default)]
pub struct OneShot {
    armed: bool,
    rearms: u32,
}

impl OneShot {
    pub fn new() -> OneShot {
        OneShot::default()
    }

    /// Программа просит картинку. `reader_in_front` — это программа на переднем плане.
    pub fn requested(&mut self, reader_in_front: bool, modifier_held: bool) -> Answer {
        if self.armed || modifier_held {
            self.armed = true;
            return Answer::GiveAndWatch;
        }
        if !reader_in_front {
            return Answer::Refuse;
        }
        if self.rearms < MAX_REARMS {
            self.rearms += 1;
            Answer::GiveAndRearm
        } else {
            Answer::Give
        }
    }

    /// Проверка по таймеру: `true` — модификатор отпущен, пора очищать буфер.
    pub fn should_clear(&self, modifier_held: bool) -> bool {
        self.armed && !modifier_held
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_v_then_release_clears() {
        let mut s = OneShot::new();
        assert!(!s.should_clear(false), "до вставки не чистим");
        assert_eq!(s.requested(true, true), Answer::GiveAndWatch);
        assert!(!s.should_clear(true), "Ctrl ещё держат — можно вставить ещё раз");
        assert_eq!(s.requested(true, true), Answer::GiveAndWatch);
        assert!(s.should_clear(false));
    }

    #[test]
    fn background_reader_gets_nothing() {
        let mut s = OneShot::new();
        for _ in 0..100 {
            assert_eq!(s.requested(false, false), Answer::Refuse);
        }
        // настоящая вставка после этого работает
        assert_eq!(s.requested(true, true), Answer::GiveAndWatch);
    }

    #[test]
    fn modifier_wins_even_if_reader_is_not_in_front() {
        // UWP: вставляет процесс приложения, а окно переднего плана — у ApplicationFrameHost
        let mut s = OneShot::new();
        assert_eq!(s.requested(false, true), Answer::GiveAndWatch);
    }

    #[test]
    fn menu_paste_rearms_then_ctrl_v_is_seen() {
        let mut s = OneShot::new();
        assert_eq!(s.requested(true, false), Answer::GiveAndRearm);
        assert!(!s.should_clear(false));
        assert_eq!(s.requested(true, true), Answer::GiveAndWatch);
        assert!(s.should_clear(false));
    }

    #[test]
    fn greedy_front_reader_cannot_loop_forever() {
        let mut s = OneShot::new();
        for _ in 0..MAX_REARMS {
            assert_eq!(s.requested(true, false), Answer::GiveAndRearm);
        }
        assert_eq!(s.requested(true, false), Answer::Give);
    }
}
