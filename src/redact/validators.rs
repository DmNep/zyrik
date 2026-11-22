//! Контрольные суммы: отличают настоящие номера и адреса от случайных цифр (меньше ложных
//! срабатываний). Карты — Луна; СНИЛС и ИНН — контрольные числа; Bitcoin — Base58Check и
//! Bech32/Bech32m; Ethereum — EIP-55 (если в адресе смешан регистр).

use sha2::{Digest, Sha256};
use sha3::Keccak256;

fn digits(s: &str) -> Vec<u32> {
    s.chars().filter_map(|c| c.to_digit(10)).collect()
}

/// Номер карты: 13–19 цифр, алгоритм Луна.
pub fn luhn(s: &str) -> bool {
    let d = digits(s);
    if !(13..=19).contains(&d.len()) {
        return false;
    }
    let sum: u32 = d
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &x)| if i % 2 == 1 { let y = x * 2; if y > 9 { y - 9 } else { y } } else { x })
        .sum();
    sum.is_multiple_of(10)
}

/// СНИЛС: 11 цифр, контрольное число из первых девяти.
pub fn snils(s: &str) -> bool {
    let d = digits(s);
    if d.len() != 11 {
        return false;
    }
    let sum: u32 = d[..9].iter().zip((1..=9).rev()).map(|(x, w)| x * w).sum();
    let check = match sum {
        0..=99 => sum,
        100 | 101 => 0,
        _ => match sum % 101 {
            100 => 0,
            r => r,
        },
    };
    check == d[9] * 10 + d[10]
}

fn inn_digit(d: &[u32], w: &[u32]) -> u32 {
    d.iter().zip(w).map(|(x, w)| x * w).sum::<u32>() % 11 % 10
}

/// ИНН: 10 цифр (организация) или 12 (человек), контрольные цифры.
pub fn inn(s: &str) -> bool {
    let d = digits(s);
    match d.len() {
        10 => inn_digit(&d, &[2, 4, 10, 3, 5, 9, 4, 6, 8]) == d[9],
        12 => {
            inn_digit(&d, &[7, 2, 4, 10, 3, 5, 9, 4, 6, 8]) == d[10]
                && inn_digit(&d, &[3, 7, 2, 4, 10, 3, 5, 9, 4, 6, 8]) == d[11]
        }
        _ => false,
    }
}

const B58: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

fn base58_decode(s: &str) -> Option<Vec<u8>> {
    let mut out: Vec<u8> = Vec::new();
    for c in s.bytes() {
        let mut carry = B58.iter().position(|&b| b == c)? as u32;
        for b in out.iter_mut().rev() {
            carry += u32::from(*b) * 58;
            *b = carry as u8;
            carry >>= 8;
        }
        while carry > 0 {
            out.insert(0, carry as u8);
            carry >>= 8;
        }
    }
    let zeros = s.bytes().take_while(|&b| b == b'1').count();
    let mut v = vec![0u8; zeros];
    v.extend(out);
    Some(v)
}

/// Base58Check: последние 4 байта — двойной SHA-256 от остального (адреса Bitcoin 1…/3…,
/// ключи WIF, расширенные ключи xprv).
pub fn base58check(s: &str) -> bool {
    let Some(b) = base58_decode(s) else { return false };
    if b.len() < 5 {
        return false;
    }
    let (payload, check) = b.split_at(b.len() - 4);
    let h = Sha256::digest(Sha256::digest(payload));
    h[..4] == *check
}

fn polymod(values: &[u8]) -> u32 {
    const GEN: [u32; 5] = [0x3b6a_57b2, 0x2650_8e6d, 0x1ea1_19fa, 0x3d42_33dd, 0x2a14_62b3];
    let mut chk = 1u32;
    for &v in values {
        let top = chk >> 25;
        chk = ((chk & 0x01ff_ffff) << 5) ^ u32::from(v);
        for (i, g) in GEN.iter().enumerate() {
            if (top >> i) & 1 == 1 {
                chk ^= g;
            }
        }
    }
    chk
}

/// Bech32 и Bech32m (адреса `bc1…`).
pub fn bech32(s: &str) -> bool {
    const CHARSET: &[u8; 32] = b"qpzry9x8gf2tvdw0s3jn54khce6mua7l";
    if s.chars().any(char::is_uppercase) && s.chars().any(char::is_lowercase) {
        return false;
    }
    let s = s.to_ascii_lowercase();
    let Some(pos) = s.rfind('1') else { return false };
    let (hrp, data) = (&s[..pos], &s[pos + 1..]);
    if hrp.is_empty() || data.len() < 6 {
        return false;
    }
    let mut values: Vec<u8> = hrp.bytes().map(|b| b >> 5).collect();
    values.push(0);
    values.extend(hrp.bytes().map(|b| b & 31));
    for c in data.bytes() {
        match CHARSET.iter().position(|&x| x == c) {
            Some(v) => values.push(v as u8),
            None => return false,
        }
    }
    matches!(polymod(&values), 1 | 0x2bc8_30a3)
}

/// Адрес Ethereum `0x` + 40 hex. Смешанный регистр — контрольная сумма EIP-55;
/// весь в одном регистре — контрольной суммы нет, считаем адресом.
pub fn eip55(s: &str) -> bool {
    let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) else { return false };
    if hex.len() != 40 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return false;
    }
    let lower = hex.to_ascii_lowercase();
    if hex == lower || hex == hex.to_ascii_uppercase() {
        return true;
    }
    let hash = Keccak256::digest(lower.as_bytes());
    hex.bytes().enumerate().all(|(i, c)| {
        let nibble = (hash[i / 2] >> (if i % 2 == 0 { 4 } else { 0 })) & 0xf;
        if c.is_ascii_digit() {
            true
        } else if nibble >= 8 {
            c.is_ascii_uppercase()
        } else {
            c.is_ascii_lowercase()
        }
    })
}

/// Энтропия Шеннона, бит на символ.
pub fn entropy(s: &str) -> f64 {
    let mut counts = [0u32; 128];
    let mut n = 0u32;
    for b in s.bytes().filter(u8::is_ascii) {
        counts[b as usize] += 1;
        n += 1;
    }
    if n == 0 {
        return 0.0;
    }
    counts
        .iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = f64::from(c) / f64::from(n);
            -p * p.log2()
        })
        .sum()
}
