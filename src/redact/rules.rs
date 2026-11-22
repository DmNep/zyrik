//! Правила по категориям спеки (история 4). Строковые — регулярные выражения по строке текста,
//! где нужно — с проверкой контрольной суммы; многострочные — блоки приватных ключей.
//! Русские и английские метки («пароль», «password») ищутся всегда, независимо от `--lang`.

use std::sync::OnceLock;

use regex::Regex;

use super::validators::{base58check, entropy, inn, luhn, snils};
use super::{Category, Finding, Line};

struct Rule {
    category: Category,
    re: Regex,
    /// Какую группу подсвечивать (0 — всё совпадение)
    group: usize,
    check: Option<fn(&str) -> bool>,
}

fn rule(category: Category, pattern: &str, group: usize, check: Option<fn(&str) -> bool>) -> Rule {
    Rule { category, re: Regex::new(pattern).expect("правило"), group, check }
}

/// IPv6: разбирается как адрес, есть цифра и `::` или все восемь групп — так `std::vector`
/// и `abc::def` из кода не считаются адресами.
fn is_ipv6(s: &str) -> bool {
    s.len() >= 6
        && s.chars().any(|c| c.is_ascii_digit())
        && (s.contains("::") || s.matches(':').count() == 7)
        && s.parse::<std::net::Ipv6Addr>().is_ok()
}

/// Значение флага `-p`/`--password` похоже на пароль: буквы и цифры, от 6 знаков
/// (так `-p 8080:80` у docker паролем не считается).
fn passwordish(s: &str) -> bool {
    s.len() >= 6 && s.chars().any(|c| c.is_ascii_alphabetic()) && s.chars().any(|c| c.is_ascii_digit()) && !s.contains(':')
}

/// Ровно 40 hex-знаков (распознавание вставляет пробелы).
fn hex40(s: &str) -> bool {
    s.chars().skip(2).filter(char::is_ascii_hexdigit).count() == 40
}

/// Случайная строка, которую распознавание могло разорвать пробелами: проверяем склеенную.
fn high_entropy_joined(s: &str) -> bool {
    let joined: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    joined.len() >= 24 && high_entropy(&joined)
}

fn wallet_base58(s: &str) -> bool {
    base58check(s)
}

fn high_entropy(s: &str) -> bool {
    let classes = [s.chars().any(|c| c.is_ascii_lowercase()), s.chars().any(|c| c.is_ascii_uppercase()), s.chars().any(|c| c.is_ascii_digit())];
    // похоже на случайный ключ: три класса символов, мало повторов, нет слов через «_»/«-» подряд
    classes.iter().all(|&c| c) && entropy(s) >= 4.0 && s.matches(['_', '-', '.']).count() <= 2
}

fn rules() -> &'static [Rule] {
    static R: OnceLock<Vec<Rule>> = OnceLock::new();
    R.get_or_init(|| {
        use Category::*;
        vec![
            // --- ключи и учётные данные
            // распознавание путает начало («sssh-») и разрывает base64 пробелами
            rule(SshKey, r"\S*(?:sh-(?:rsa|ed25519|dss)|ecdsa-sha2-nistp\d+)\s+AAAA[0-9A-Za-z+/=]{6,}(?:\s[0-9A-Za-z+/=]{4,})*", 0, None),
            rule(Token, r"\b(?:gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{22,}|glpat-[A-Za-z0-9_-]{20,}|AKIA[0-9A-Z]{16}|ASIA[0-9A-Z]{16}|AIza[0-9A-Za-z_-]{35}|xox[abposr]\s?-\s?(?:[0-9A-Za-z]+\s?-\s?){2,}[0-9A-Za-z]{6,}|sk-(?:proj-|ant-)?[A-Za-z0-9_-]{20,}|[sr]k[_ ](?:live|test)[_ ][0-9A-Za-z]{16,}|eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,})", 0, None),
            rule(Token, r"(?i)\bBearer\s+([A-Za-z0-9._~+/-]{16,}=*)", 1, None),
            // распознавание вставляет пробелы: «postgres: //app: пароль@db. internal :5432/app»
            rule(UrlCredentials, r"\b[a-zA-Z][a-zA-Z0-9+.-]*:\s?//\s?[^\s:/@]+:\s?[^\s@/]+@[^\s/]+(?:\s?[.:]\s?[^\s/]+)*(?:/\S*)?", 0, None),
            rule(ConnectionString, r"(?i)\bSrvr\s*=.*(?:Usr|Pwd)\s*=[^;\s]*", 0, None),
            rule(Password, r"(?i)(?:^|[\s;,{(]|\b)(?:password|passwd|pwd|pass|secret|token|api[_-]?key|access[_-]?key|пароль|ключ)[\w-]{0,20}\s*[:=]\s*(\S+)", 1, None),
            // переменная окружения с секретом: значение до конца строки (распознавание рвёт его пробелами)
            rule(Password, r"\b[A-Z][A-Z0-9_]*(?:SECRET|TOKEN|PASSWORD|PASSWD|API_KEY|PRIVATE_KEY)[A-Z0-9_]*\s*=\s*(\S.*?)\s*$", 1, None),
            rule(Password, r"(?:^|\s)(?:-p|--password(?:=|\s))\s*(\S+)", 1, Some(passwordish)),
            // --- криптокошельки
            rule(CryptoKey, r"\b[xyzt]prv[1-9A-HJ-NP-Za-km-z]{100,112}\b", 0, Some(base58check)),
            rule(CryptoKey, r"\b[5KL][1-9A-HJ-NP-Za-km-z]{50,51}\b", 0, Some(base58check)),
            rule(CryptoKey, r"\b(?:0x)?[0-9a-fA-F]{64}\b", 0, None),
            // контрольную сумму EIP-55 не требуем: распознавание путает регистр, а пропуск хуже
            rule(Wallet, r"\b[0O][xX](?:\s?[0-9a-fA-F]){40}\b", 0, Some(hex40)),
            // контрольную сумму Bech32 не требуем: одна ошибка распознавания её ломает
            rule(Wallet, r"\b(?:bc|tb|ltc)1(?:\s?[0-9a-zA-Z]){25,87}\b", 0, None),
            rule(Wallet, r"\b[13][1-9A-HJ-NP-Za-km-z]{25,34}\b", 0, Some(wallet_base58)),
            // --- личное
            rule(Email, r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}\b", 0, None),
            rule(Card, r"\b(?:\d[ -]?){12,18}\d\b", 0, Some(luhn)),
            rule(Snils, r"\b\d{3}-\d{3}-\d{3}[ -]\d{2}\b", 0, Some(snils)),
            rule(Inn, r"(?i)(?:ИНН|INN)\s*:?\s*(\d{10}|\d{12})\b", 1, Some(inn)),
            rule(Inn, r"\b\d{12}\b", 0, Some(inn)),
            rule(Passport, r"(?i)паспорт\D{0,20}(\d{2}\s?\d{2}\s?№?\s?\d{6})\b", 1, None),
            rule(Passport, r"\b\d{2} \d{2} \d{6}\b", 0, None),
            rule(BankAccount, r"\b(?:40[5-8]|423|426|30101|30102|40817)\d{15,17}\b", 0, None),
            rule(Phone, r"(?:\+7|\b8)[\s(-]*\d{3}[\s)-]*\d{3}[\s-]*\d{2}[\s-]*\d{2}\b", 0, None),
            rule(Phone, r"\+\d{1,3}[\s(-]*\d{2,4}[\s)-]*\d{2,4}[\s-]*\d{2,4}(?:[\s-]*\d{2,4})?\b", 0, None),
            rule(Name, r"\b[А-ЯЁ][а-яё]+ [А-ЯЁ][а-яё]+ [А-ЯЁ][а-яё]+(?:вич|вна|ична|инична|оглы|кызы)\b", 0, None),
            rule(Name, r"\b[А-ЯЁ][а-яё]{2,} [А-ЯЁ]\.\s?[А-ЯЁ]\.", 0, None),
            // --- инфраструктура
            rule(UserPath, r"(?i)\b[A-Z]:\\Users\\[^\\\s]+", 0, None),
            rule(UserPath, r"(?:^|[\s:='\x22(])(/home/[^/\s]+|/Users/[^/\s]+)", 1, None),
            rule(UserPath, r"\\\\[A-Za-z0-9._-]+\\[^\s]+", 0, None),
            rule(Mac, r"\b[0-9A-Fa-f]{2}(?:[:-][0-9A-Fa-f]{2}){5}\b", 0, None),
            rule(Ip, r"\b(?:(?:25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)\.){3}(?:25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)\b", 0, None),
            rule(Ip, r"\b[0-9A-Fa-f]{0,4}(?::[0-9A-Fa-f]{0,4}){2,7}\b", 0, Some(is_ipv6)),
            rule(Host, r"(?i)\b[a-z0-9-]+(?:\.[a-z0-9-]+)*\.(?:local|lan|corp|internal|intranet|home\.arpa)\b", 0, None),
            // --- длинные случайные строки, не попавшие в шаблоны выше
            rule(RandomSecret, r"[A-Za-z0-9+/=_-]{24,}", 0, Some(high_entropy)),
            rule(RandomSecret, r"[A-Za-z0-9+/=_{}\\-]{6,}(?:\s[A-Za-z0-9+/=_{}\\-]{4,}){1,4}", 0, Some(high_entropy_joined)),
        ]
    })
}

/// Байтовое смещение → номер символа.
fn char_index(text: &str, byte: usize) -> usize {
    text[..byte].chars().count()
}

pub(crate) fn line_rules(lines: &[Line]) -> Vec<Finding> {
    let mut out = Vec::new();
    for line in lines {
        let mut taken: Vec<(usize, usize)> = Vec::new();
        for r in rules() {
            for caps in r.re.captures_iter(&line.text) {
                let Some(m) = caps.get(r.group) else { continue };
                if let Some(check) = r.check {
                    if !check(m.as_str()) {
                        continue;
                    }
                }
                let (a, b) = (char_index(&line.text, m.start()), char_index(&line.text, m.end()));
                // одно место — одна категория: первое (более точное) правило побеждает
                if taken.iter().any(|&(x, y)| a < y && x < b) {
                    continue;
                }
                taken.push((a, b));
                if let Some(rect) = line.rect(a, b) {
                    out.push(Finding { category: r.category, rects: vec![rect] });
                }
            }
        }
    }
    out
}

/// Приватные ключи: от строки `-----BEGIN … PRIVATE KEY-----` (и `PGP PRIVATE KEY BLOCK`)
/// до `-----END …-----` — целиком, все строки между ними.
pub(crate) fn private_key_blocks(lines: &[Line]) -> Vec<Finding> {
    let begin = |t: &str| t.contains("BEGIN") && t.contains("PRIVATE KEY") && t.contains("-----");
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if !begin(&lines[i].text) {
            i += 1;
            continue;
        }
        let start = i;
        // конец — строка с END; если её не видно (обрезано зоной) — до последней строки base64
        let mut end = i;
        for (j, l) in lines.iter().enumerate().skip(i + 1) {
            end = j;
            if l.text.contains("END") && l.text.contains("-----") {
                break;
            }
            let base64ish = l.text.chars().all(|c| c.is_ascii_alphanumeric() || "+/=: -".contains(c));
            if !base64ish {
                end = j - 1;
                break;
            }
        }
        let rects = lines[start..=end].iter().filter_map(Line::full).collect();
        out.push(Finding { category: Category::PrivateKey, rects });
        i = end + 1;
    }
    out
}
