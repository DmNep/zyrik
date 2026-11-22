//! Сид-фразы: 12, 15, 18, 21 или 24 слова подряд из английского словаря BIP-39 (встроен,
//! `assets/bip39-english.txt`, MIT). Порядковые номера («1.», «2)») между словами не мешают;
//! фраза может переноситься на следующие строки — подсвечивается целиком.

use std::collections::HashSet;
use std::sync::OnceLock;

use super::{Category, Finding, Line};
use crate::session::Rect;

static LIST: &str = include_str!("../../assets/bip39-english.txt");

fn words() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| LIST.lines().map(str::trim).filter(|w| !w.is_empty()).collect())
}

pub fn is_word(w: &str) -> bool {
    words().contains(w)
}

/// Слово без знаков по краям, в нижнем регистре; номер вида «1.», «12)» → `None` (пропустить).
fn clean(w: &str) -> Option<Option<String>> {
    let t: String = w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
    // распознавание склеивает номер со словом: «1.now», «4,sell»
    let t = match t.find(|c: char| !c.is_ascii_digit()) {
        Some(i) if i > 0 && t[i..].starts_with(['.', ',', ')']) => t[i + 1..].trim_start().to_string(),
        _ => t,
    };
    if t.is_empty() || t.chars().all(|c| c.is_ascii_digit()) {
        return Some(None); // номер или знак — пропускаем, фразу не рвём
    }
    Some(Some(t))
}

const MIN: usize = 12;

pub(crate) fn seed_phrases(lines: &[Line]) -> Vec<Finding> {
    // все слова по порядку: (строка, индекс слова в строке, слово словаря или нет)
    let mut seq: Vec<(usize, usize, bool)> = Vec::new();
    for (li, l) in lines.iter().enumerate() {
        let line: Vec<(usize, usize, bool)> = l
            .words
            .iter()
            .enumerate()
            .filter_map(|(wi, w)| match clean(&w.text) {
                Some(None) => None,
                Some(Some(t)) => Some((li, wi, is_word(&t))),
                None => Some((li, wi, false)),
            })
            .collect();
        // строка без единого слова словаря (соседнее окно, колонка) фразу не рвёт
        if line.iter().any(|x| x.2) {
            seq.extend(line);
        }
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < seq.len() {
        if !seq[i].2 {
            i += 1;
            continue;
        }
        let start = i;
        while i < seq.len() && seq[i].2 {
            i += 1;
        }
        if i - start >= MIN {
            out.push(Finding { category: Category::SeedPhrase, rects: rects(lines, &seq[start..i]) });
        }
    }
    out
}

/// По прямоугольнику на строку, которую занимает фраза.
fn rects(lines: &[Line], run: &[(usize, usize, bool)]) -> Vec<Rect> {
    let mut out: Vec<Rect> = Vec::new();
    let mut li_prev = usize::MAX;
    for &(li, wi, _) in run {
        let r = lines[li].words[wi].rect;
        if li == li_prev {
            let last = out.last_mut().expect("есть");
            *last = super::union(*last, r);
        } else {
            out.push(r);
            li_prev = li;
        }
    }
    out
}
