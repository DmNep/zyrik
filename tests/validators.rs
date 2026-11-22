//! T061: контрольные суммы — настоящие примеры проходят, испорченные нет.

use zyrik::redact::validators::*;

#[test]
fn luhn_cards() {
    assert!(luhn("4111 1111 1111 1111"));
    assert!(luhn("5500-0000-0000-0004"));
    assert!(luhn("2200 0000 0000 0004"), "Мир");
    assert!(!luhn("4111 1111 1111 1112"));
    assert!(!luhn("1234"), "слишком коротко");
}

#[test]
fn snils_checksum() {
    assert!(snils("112-233-445 95"));
    assert!(!snils("112-233-445 96"));
    assert!(!snils("112-233-445"));
}

#[test]
fn inn_checksum() {
    assert!(inn("7707083893"), "ИНН организации");
    assert!(!inn("7707083894"));
    assert!(inn("500100732259"), "ИНН человека");
    assert!(!inn("500100732258"));
    assert!(!inn("12345"));
}

#[test]
fn base58check_addresses_and_keys() {
    assert!(base58check("1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa"), "адрес генезис-блока");
    assert!(!base58check("1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNb"));
    assert!(base58check("5HueCGU8rMjxEXxiPuD5BDku4MkFqeZyd4dZ1jvhTVqvbTLvyTJ"), "WIF из примера bitcoin wiki");
    assert!(!base58check("0OIl"), "символы вне алфавита");
}

#[test]
fn bech32_and_bech32m() {
    assert!(bech32("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4"), "BIP-173");
    assert!(bech32("BC1QW508D6QEJXTDG4Y5R3ZARVARY0C5XW7KV8F3T4"), "в верхнем регистре тоже");
    assert!(bech32("bc1p0xlxvlhemja6c4dqv22uapctqupfhlxm9h8z3k2e72q4k9hcz7vqzk5jj0"), "BIP-350, Bech32m");
    assert!(!bech32("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t5"));
    assert!(!bech32("bc1Qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4"), "смешанный регистр запрещён");
}

#[test]
fn eip55_addresses() {
    assert!(eip55("0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed"), "пример из EIP-55");
    assert!(eip55("0xfB6916095ca1df60bB79Ce92cE3Ea74c37c5d359"));
    assert!(eip55("0x5aaeb6053f3e94c9b9a09f33669435e7ef1beaed"), "без контрольной суммы — в одном регистре");
    assert!(!eip55("0x5AAeb6053F3E94C9b9A09f33669435E7Ef1BeAed"), "одна буква не в том регистре");
    assert!(!eip55("0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAe"), "39 символов");
}

#[test]
fn entropy_tells_random_from_words() {
    assert!(entropy("aB3xK9qLm2Zp7RtY4wVn8sDf") > 4.0);
    assert!(entropy("aaaaaaaaaaaaaaaaaaaaaaaa") < 0.1);
    assert!(entropy("passwordpasswordpassword") < 3.0, "повторы — мало энтропии");
}
