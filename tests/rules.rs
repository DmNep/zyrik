//! T064: поиск «опасного» по словам с рамками — по категориям спеки, и что обычный код и
//! документация чистые (SC-008). Слова раскладываются как моноширинный текст: 10 px на символ.

use zyrik::redact::{detect, Category, Finding, Word};
use zyrik::session::Rect;

const CW: i32 = 10;
const LH: i32 = 20;

fn words(text: &str) -> Vec<Word> {
    let mut out = Vec::new();
    for (li, line) in text.lines().enumerate() {
        let mut col = 0usize;
        for part in line.split(' ') {
            let n = part.chars().count();
            if n > 0 {
                out.push(Word {
                    text: part.to_string(),
                    rect: Rect { x: col as i32 * CW, y: li as i32 * LH, w: n as i32 * CW, h: 16 },
                    line: li,
                });
            }
            col += n + 1;
        }
    }
    out
}

fn find(text: &str) -> Vec<Finding> {
    detect(&words(text))
}

fn cats(text: &str) -> Vec<Category> {
    find(text).iter().map(|f| f.category).collect()
}

/// Подсвеченный текст строки `line` по рамке находки (обратно из пикселей в символы).
fn covered(text: &str, f: &Finding, line: usize) -> String {
    let l: Vec<char> = text.lines().nth(line).unwrap().chars().collect();
    let r = f.rects.iter().find(|r| r.y <= line as i32 * LH && r.bottom() >= line as i32 * LH + 16).unwrap();
    let a = ((r.x + 2) / CW).max(0) as usize;
    let b = (((r.right() - 2) + CW - 1) / CW) as usize;
    l[a.min(l.len())..b.min(l.len())].iter().collect()
}

fn one(text: &str, cat: Category) -> Finding {
    let f = find(text);
    assert_eq!(f.len(), 1, "{text:?}: {f:?}");
    assert_eq!(f[0].category, cat, "{text:?}");
    f[0].clone()
}

// ---------- личное ----------

#[test]
fn email_phone_card() {
    let t = "Пишите на ivan.petrov@example.com по вопросам";
    let f = one(t, Category::Email);
    assert_eq!(covered(t, &f, 0), "ivan.petrov@example.com");
    one("тел. +7 (912) 345-67-89", Category::Phone);
    one("звоните 8 912 345 67 89", Category::Phone);
    let t = "карта 4111 1111 1111 1111 до 12/27";
    let f = one(t, Category::Card);
    assert_eq!(covered(t, &f, 0), "4111 1111 1111 1111");
    assert!(!cats("номер 4111 1111 1111 1112").contains(&Category::Card), "не проходит Луну");
}

#[test]
fn russian_documents() {
    one("СНИЛС 112-233-445 95", Category::Snils);
    assert!(find("СНИЛС 112-233-445 96").is_empty(), "контрольное число не сходится");
    one("ИНН: 7707083893", Category::Inn);
    one("ИНН 500100732259", Category::Inn);
    one("паспорт серия 45 12 № 345678", Category::Passport);
    one("р/с 40817810099910004312", Category::BankAccount);
}

#[test]
fn names() {
    one("Исполнитель: Петров Иван Сергеевич", Category::Name);
    one("подписал Сидорова А.В.", Category::Name);
}

// ---------- ключи и учётные данные ----------

#[test]
fn private_key_block_is_one_finding_over_all_lines() {
    let t = "id_rsa:\n-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW\nQyNTUxOQAAACDiJ3Vw7Rk4Gx6q8w0m5X0bqY2K7n4vRrF0c8bT2YkDLQAAAJgVv3Q1Fb90\n-----END OPENSSH PRIVATE KEY-----\nготово";
    let f = find(t);
    assert_eq!(f.len(), 1, "{f:?}");
    assert_eq!(f[0].category, Category::PrivateKey);
    assert_eq!(f[0].rects.len(), 4, "четыре строки блока");
    assert!(f[0].rects.iter().all(|r| r.y >= LH - 2 && r.bottom() <= 5 * LH), "без «id_rsa:» и «готово»");
}

#[test]
fn ssh_public_key() {
    one("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIHk3+2qW8mZ4b5R7lN0aPq user@laptop", Category::SshKey);
}

#[test]
fn tokens() {
    for t in [
        "token ghp_aB3dE5fG7hJ9kL1mN3pQ5rS7tU9vW1xY3zA5",
        "export AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE",
        "key=AIzaSyA1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6Q",
        "xoxb-123456789012-1234567890123-AbCdEfGhIjKlMnOpQrStUvWx",
        "OPENAI=sk-proj-a1B2c3D4e5F6g7H8i9J0k1L2m3N4o5P6",
        "Authorization: Bearer abcDEF123ghiJKL456mnoPQR789",
        "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U",
    ] {
        assert!(!find(t).is_empty(), "не найдено: {t}");
    }
}

#[test]
fn passwords_highlight_only_the_value() {
    let t = "DB_PASSWORD=hunter2secret";
    let f = one(t, Category::Password);
    assert_eq!(covered(t, &f, 0), "hunter2secret");
    let t = "пароль: Qwerty123";
    let f = one(t, Category::Password);
    assert_eq!(covered(t, &f, 0), "Qwerty123");
    one("password = correct-horse", Category::Password);
}

#[test]
fn credentials_in_urls_and_connection_strings() {
    one("postgres://admin:S3cr3t@db.example.com:5432/app", Category::UrlCredentials);
    one("Server=sql01;Database=erp;User Id=sa;Password=Pa55w0rd;", Category::Password);
    one("Srvr=\"1c-srv\";Ref=\"buh\";Usr=\"Иванов\";Pwd=\"123\";", Category::ConnectionString);
}

#[test]
fn random_high_entropy_string() {
    one("secret_value: Zk8qL2mX9vB4nR7tY1wP5sD3", Category::Password);
    one("id Zk8qL2mX9vB4nR7tY1wP5sD3gH6", Category::RandomSecret);
}

// ---------- криптокошельки ----------

#[test]
fn seed_phrase_across_lines_and_numbers() {
    let t = "Сохраните фразу:\n1. abandon 2. ability 3. able 4. about\n5. above 6. absent 7. absorb 8. abstract\n9. absurd 10. abuse 11. access 12. accident\nникому не показывайте";
    let f = find(t);
    assert_eq!(f.len(), 1, "{f:?}");
    assert_eq!(f[0].category, Category::SeedPhrase);
    assert_eq!(f[0].rects.len(), 3, "три строки фразы");
    assert!(find("abandon ability able about above absent absorb abstract absurd abuse access").is_empty(), "11 слов — не фраза");
}

#[test]
fn crypto_keys_and_wallets() {
    one("priv 5HueCGU8rMjxEXxiPuD5BDku4MkFqeZyd4dZ1jvhTVqvbTLvyTJ", Category::CryptoKey);
    one("0x4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318", Category::CryptoKey);
    one("send to 0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed", Category::Wallet);
    one("btc: bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4", Category::Wallet);
    one("1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa", Category::Wallet);
    // EIP-55 не требуется: распознавание текста путает регистр букв, а пропустить адрес хуже
    one("0x5AAeb6053F3E94C9b9A09f33669435E7Ef1BeAed", Category::Wallet);
}

// ---------- инфраструктура ----------

#[test]
fn infrastructure() {
    one("ssh root@10.20.30.40", Category::Ip);
    one("gateway fe80::1ff:fe23:4567:890a", Category::Ip);
    one("ether 00:1a:2b:3c:4d:5e", Category::Mac);
    one("ping nas.corp", Category::Host);
    let t = r"C:\Users\ivanov\Documents\otchet.docx";
    let f = one(t, Category::UserPath);
    assert_eq!(covered(t, &f, 0), r"C:\Users\ivanov");
    one("cd /home/petrov/projects", Category::UserPath);
    one(r"\\fileserver\share\buh", Category::UserPath);
}

// ---------- чистые тексты (SC-008) ----------

#[test]
fn ordinary_code_and_docs_are_clean() {
    let samples = [
        "use std::sync::Arc;\nfn main() {\n    let v: Vec<u32> = Vec::new();\n    println!(\"{}\", v.len());\n}",
        "impl<T: Clone> Iterator for Wrapper<T> {\n    type Item = T;\n    fn next(&mut self) -> Option<T> { None }\n}",
        "def handle_request(self, request):\n    user_id = request.args.get('user_id')\n    return render_template('index.html', items=items)",
        "Версия 1.2.3 вышла 21.11.2026 в 14:05:33. Подробности в CHANGELOG.md.",
        "Нажмите Ctrl+Shift+Esc, чтобы открыть диспетчер задач. Затем выберите вкладку «Процессы».",
        "The quick brown fox jumps over the lazy dog. Lorem ipsum dolor sit amet, consectetur adipiscing elit.",
        "for (int i = 0; i < 100; i++) { sum += arr[i] * 2; }",
        "git commit -m \"Fix bug in parser\" && git push origin main",
        "Размер файла 1048576 байт, 1920x1080, 60 Гц, цена 1 299 руб.",
        "std::vector<std::string> names; abc::def(); http://example.com/docs/index.html",
        "SELECT id, name FROM users WHERE created_at > '2026-01-01' ORDER BY id LIMIT 100;",
    ];
    for s in samples {
        let f = find(s);
        assert!(f.len() <= 1, "SC-008: больше одного ложного срабатывания в {s:?}: {f:?}");
        assert!(f.is_empty(), "ложное срабатывание в {s:?}: {f:?}");
    }
}

#[test]
fn far_apart_words_on_one_row_are_separate_lines() {
    // распознавание склеило сид-фразу с текстом соседнего окна далеко справа
    let mut ws = words("1. abandon 2. ability 3. able 4. about 5. above 6. absent
7. absorb 8. abstract 9. absurd 10. abuse 11. access 12. accident");
    for (i, extra) in ["ieee", "pulses/min"].iter().enumerate() {
        ws.push(Word { text: extra.to_string(), rect: Rect { x: 1500, y: i as i32 * LH, w: 40, h: 16 }, line: i });
    }
    let f = detect(&ws);
    assert!(f.iter().any(|f| f.category == Category::SeedPhrase), "{f:?}");
}

#[test]
fn rows_come_from_geometry_not_from_recognizer_line_ids() {
    // распознавание выдало обе строки фразы как одну (line = 0) и ещё слово соседнего окна
    let mut ws = words("1. abandon 2. ability 3. able 4. about 5. above 6. absent
7. absorb 8. abstract 9. absurd 10. abuse 11. access 12. accident
fn main() { }");
    for w in ws.iter_mut().filter(|w| w.rect.y < 2 * LH) {
        w.line = 0;
    }
    ws.push(Word { text: "oeescn".into(), rect: Rect { x: 1500, y: 4, w: 60, h: 30 }, line: 0 });
    let f = detect(&ws);
    let seed: Vec<&Finding> = f.iter().filter(|f| f.category == Category::SeedPhrase).collect();
    assert_eq!(seed.len(), 1, "{f:?}");
    assert_eq!(seed[0].rects.len(), 2, "две строки фразы");
}
