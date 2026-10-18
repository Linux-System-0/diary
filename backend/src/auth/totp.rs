use anyhow::{Context, Result};
use rand::{RngCore, rngs::OsRng};
use totp_rs::{Algorithm, Builder, Totp};

const STEP_SECS: u64 = 30;
const DIGITS: u8 = 6;
const SECRET_LEN: usize = 20;

pub fn generate_secret() -> [u8; SECRET_LEN] {
    let mut bytes = [0u8; SECRET_LEN];
    OsRng.fill_bytes(&mut bytes);
    bytes
}

pub fn build(secret: &[u8], username: &str, skew: u8) -> Result<Totp> {
    Builder::new()
        .with_algorithm(Algorithm::SHA1)
        .with_digits(DIGITS)
        .with_skew(skew as u16)
        .with_step_duration(STEP_SECS)
        .with_secret(secret.to_vec())
        .with_issuer(Some("diary"))
        .with_account_name(username)
        .build()
        .context("TOTP 参数无效")
}

pub fn secret_base32(totp: &Totp) -> String {
    totp.secret().to_base32()
}

pub fn otpauth_uri(totp: &Totp) -> Result<String> {
    totp.to_url().context("生成 otpauth URI 失败")
}

/// Removes whitespace and any non-digit characters, then requires exactly six
/// digits. Invalid input is rejected before touching replay state.
pub fn normalize_code(input: &str) -> Option<String> {
    let digits: String = input.chars().filter(char::is_ascii_digit).collect();
    (digits.len() == DIGITS as usize).then_some(digits)
}

/// Returns the accepted time-step when the code is valid. The caller must enforce
/// `step > totp_last_step` to prevent replay within the skew window.
pub fn check_now(totp: &Totp, input: &str) -> Option<u64> {
    let code = normalize_code(input)?;
    totp.check_current(&code)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"01234567890123456789";

    #[test]
    fn normalizes_input() {
        assert_eq!(normalize_code(" 123 456 "), Some("123456".into()));
        assert_eq!(normalize_code("12345"), None);
        assert_eq!(normalize_code("1234567"), None);
        assert_eq!(normalize_code("abcdef"), None);
    }

    #[test]
    fn accepts_current_code_and_adjacent_steps() {
        let totp = build(SECRET, "alice", 1).unwrap();
        let now = 1_700_000_000;
        let code = totp.generate(now).to_string();
        assert!(totp.check(&code, now).is_some());
        assert!(totp.check(&code, now + 30).is_some());
        assert!(totp.check(&code, now + 31).is_some());
        assert!(totp.check(&code, now + 60).is_none());
        assert!(totp.check(&code, now - 30).is_some());
        assert!(totp.check(&code, now - 31).is_some());
        assert!(totp.check(&code, now - 60).is_none());
    }

    #[test]
    fn rejects_wrong_code() {
        let totp = build(SECRET, "alice", 1).unwrap();
        assert!(totp.check("000000", 1_700_000_000).is_none());
    }
}
