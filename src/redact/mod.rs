//! Поиск «опасного» на снимке (история 4): слова с рамками (их даёт распознавание текста) →
//! совпадения правил → прямоугольники с категорией. Без нейросети: шаблоны, контрольные суммы,
//! встроенный словарь BIP-39 (как у Зябры). Подсветка — помощь, а не гарантия.

pub mod bip39;
pub mod rules;
pub mod validators;

use crate::session::Rect;

/// Слово, распознанное на снимке. `line` — номер строки в порядке чтения.
#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub text: String,
    pub rect: Rect,
    pub line: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    Name,
    Email,
    Phone,
    Card,
    BankAccount,
    Passport,
    Snils,
    Inn,
    PrivateKey,
    SshKey,
    Token,
    Password,
    UrlCredentials,
    ConnectionString,
    RandomSecret,
    SeedPhrase,
    CryptoKey,
    Wallet,
    Ip,
    Mac,
    Host,
    UserPath,
}

/// Найденный фрагмент: по прямоугольнику на каждую строку, которую он занимает.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub category: Category,
    pub rects: Vec<Rect>,
}

impl Finding {
    pub fn contains(&self, x: i32, y: i32) -> bool {
        self.rects.iter().any(|r| r.contains(crate::session::Pt { x, y }))
    }

    pub fn bounds(&self) -> Rect {
        let l = self.rects.iter().map(|r| r.x).min().unwrap_or(0);
        let t = self.rects.iter().map(|r| r.y).min().unwrap_or(0);
        let r = self.rects.iter().map(|r| r.right()).max().unwrap_or(0);
        let b = self.rects.iter().map(|r| r.bottom()).max().unwrap_or(0);
        Rect { x: l, y: t, w: r - l, h: b - t }
    }
}

/// Строка текста: слова и где каждое начинается и кончается в собранной строке (в символах).
pub(crate) struct Line<'a> {
    pub words: Vec<&'a Word>,
    pub text: String,
    /// (начало, конец) каждого слова в `text`, в символах
    pub spans: Vec<(usize, usize)>,
}

/// Строки в порядке чтения — по геометрии слов, а не по номерам строк распознавания: оно иногда
/// склеивает соседние строки в одну (а то и с текстом соседнего окна), и слова перемешиваются.
/// Один ряд — слова, у которых середины по высоте близки (в пределах половины высоты); внутри
/// ряда — слева направо; разрыв больше трёх высот текста — уже другая колонка.
pub(crate) fn lines(words: &[Word]) -> Vec<Line<'_>> {
    let center = |w: &Word| w.rect.y * 2 + w.rect.h;
    let mut ws: Vec<&Word> = words.iter().collect();
    ws.sort_by_key(|w| (center(w), w.rect.x));
    // слишком высокие рамки (распознавание склеило две строки) не задают высоту ряда
    let mut heights: Vec<i32> = ws.iter().map(|w| w.rect.h.max(1)).collect();
    heights.sort_unstable();
    let typical = heights.get(heights.len() / 2).copied().unwrap_or(16);
    let mut rows: Vec<Vec<&Word>> = Vec::new();
    for w in ws {
        let h = w.rect.h.min(typical * 2).max(1);
        match rows.last_mut() {
            Some(row) if (center(w) - center(row[0])).abs() <= h => row.push(w),
            _ => rows.push(vec![w]),
        }
    }
    let mut out = Vec::new();
    for mut row in rows {
        row.sort_by_key(|w| w.rect.x);
        let mut part: Vec<&Word> = Vec::new();
        for w in row {
            if let Some(prev) = part.last() {
                let gap = w.rect.x - prev.rect.right();
                let height = prev.rect.h.min(w.rect.h).clamp(1, typical * 2);
                if gap > 3 * height {
                    out.push(line(std::mem::take(&mut part)));
                }
            }
            part.push(w);
        }
        if !part.is_empty() {
            out.push(line(part));
        }
    }
    out
}

fn line(ws: Vec<&Word>) -> Line<'_> {
    let mut text = String::new();
    let mut spans = Vec::new();
    for w in &ws {
        if !text.is_empty() {
            text.push(' ');
        }
        let start = text.chars().count();
        text.push_str(&w.text);
        spans.push((start, start + w.text.chars().count()));
    }
    Line { words: ws, text, spans }
}

impl Line<'_> {
    /// Прямоугольник под символами [from, to) строки: целые слова — их рамки, часть слова —
    /// доля рамки по числу символов (для моноширинного текста точно, для остального — близко).
    pub fn rect(&self, from: usize, to: usize) -> Option<Rect> {
        let mut out: Option<Rect> = None;
        for (w, &(s, e)) in self.words.iter().zip(&self.spans) {
            let (a, b) = (from.max(s), to.min(e));
            if a >= b {
                continue;
            }
            let len = (e - s).max(1) as i32;
            let r = w.rect;
            let x0 = r.x + r.w * (a - s) as i32 / len;
            let x1 = r.x + r.w * (b - s) as i32 / len;
            let part = Rect { x: x0, y: r.y, w: (x1 - x0).max(1), h: r.h };
            out = Some(match out {
                None => part,
                Some(o) => union(o, part),
            });
        }
        out
    }

    pub fn full(&self) -> Option<Rect> {
        self.rect(0, self.text.chars().count())
    }
}

pub(crate) fn union(a: Rect, b: Rect) -> Rect {
    let l = a.x.min(b.x);
    let t = a.y.min(b.y);
    let r = a.right().max(b.right());
    let btm = a.bottom().max(b.bottom());
    Rect { x: l, y: t, w: r - l, h: btm - t }
}

fn overlaps(a: Rect, b: Rect) -> bool {
    a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
}

/// Отступ вокруг рамки, чтобы закраска накрывала буквы целиком (выносные элементы, сглаживание).
const PAD: i32 = 2;

/// Опасное по нескольким распознаваниям одного снимка (английская и русская модели): находки
/// складываются, совпадающие по месту не дублируются — остаётся первая.
pub fn detect_all(sets: &[Vec<Word>]) -> Vec<Finding> {
    let mut out: Vec<Finding> = Vec::new();
    for words in sets {
        for f in detect(words) {
            if !out.iter().any(|o| o.rects.iter().any(|a| f.rects.iter().any(|b| overlaps(*a, *b)))) {
                out.push(f);
            }
        }
    }
    out
}

/// Всё опасное среди слов. Перекрывающиеся находки не дублируются: остаётся первая по
/// порядку правил (многострочные блоки → точные шаблоны → общие).
pub fn detect(words: &[Word]) -> Vec<Finding> {
    let lines = lines(words);
    let mut found = Vec::new();
    found.extend(rules::private_key_blocks(&lines));
    found.extend(bip39::seed_phrases(&lines));
    found.extend(rules::line_rules(&lines));
    let mut out: Vec<Finding> = Vec::new();
    for f in found {
        if out.iter().any(|o| o.rects.iter().any(|a| f.rects.iter().any(|b| overlaps(*a, *b)))) {
            continue;
        }
        out.push(f);
    }
    for f in &mut out {
        for r in &mut f.rects {
            *r = Rect { x: r.x - PAD, y: r.y - PAD, w: r.w + 2 * PAD, h: r.h + 2 * PAD };
        }
    }
    out
}
