use argon2::{
    Algorithm, Argon2, Params, PasswordHash, PasswordHasher, PasswordVerifier, Version,
    password_hash::{SaltString, rand_core::OsRng},
};

/// OWASP 2024 baseline for Argon2id: m=19456 KiB, t=2, p=1.
const M_COST: u32 = 19_456;
const T_COST: u32 = 2;
const P_COST: u32 = 1;

fn argon2() -> Argon2<'static> {
    let params = Params::new(M_COST, T_COST, P_COST, None).expect("Argon2 参数合法");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    argon2()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| anyhow::anyhow!("密码哈希失败: {error}"))
}

pub fn verify_password(password: &str, stored_hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(stored_hash) else {
        return false;
    };
    argon2().verify_password(password.as_bytes(), &parsed).is_ok()
}

/// Returns true when an existing hash uses weaker/older parameters and should be
/// transparently upgraded after a successful login.
pub fn password_needs_rehash(stored_hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(stored_hash) else {
        return true;
    };
    let Ok(params) = Params::try_from(&parsed) else {
        return true;
    };
    params.m_cost() != M_COST || params.t_cost() != T_COST || params.p_cost() != P_COST
}

pub fn validate_password(password: &str) -> Result<(), &'static str> {
    let len = password.chars().count();
    if len < 8 {
        return Err("密码至少 8 个字符");
    }
    if len > 128 {
        return Err("密码最多 128 个字符");
    }
    if is_common_password(password) {
        return Err("密码过于常见，请换一个");
    }
    Ok(())
}

/// A compact built-in weak-password list. Kept in code so validation works
/// offline and never depends on an external service.
fn is_common_password(password: &str) -> bool {
    const COMMON: &[&str] = &[
        "12345678", "123456789", "1234567890", "password", "password1", "password123",
        "qwerty123", "qwertyuiop", "11111111", "00000000", "abc12345", "letmein1",
        "iloveyou", "admin123", "welcome1", "monkey123", "dragon123", "sunshine",
        "princess", "football", "baseball", "shadow123", "master123", "superman",
        "michael123", "jennifer", "trustno1", "passw0rd", "p@ssw0rd", "qazwsx123",
        "1q2w3e4r", "1qaz2wsx", "zaq12wsx", "asdfghjk", "1234qwer", "qwer1234",
        "a1234567", "changeme", "secret123", "whatever", "starwars", "computer",
        "internet", "samsung1", "google123", "facebook", "linkedin", "instagram",
        "diary123", "diary1234",
    ];
    let normalized = password.trim().to_ascii_lowercase();
    COMMON.contains(&normalized.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_and_verifies() {
        let hash = hash_password("correct horse battery staple").unwrap();
        assert!(verify_password("correct horse battery staple", &hash));
        assert!(!verify_password("wrong password", &hash));
        assert!(!password_needs_rehash(&hash));
    }

    #[test]
    fn rejects_short_and_common() {
        assert!(validate_password("short").is_err());
        assert!(validate_password("Password1").is_err());
        assert!(validate_password("a much longer passphrase").is_ok());
    }
}
