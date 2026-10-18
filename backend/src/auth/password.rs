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
    argon2()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
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

/// Top-1000 weak passwords, embedded at compile time so validation works
/// offline. Source: SecLists, `xato-net-10-million-passwords-1000.txt` (MIT).
fn is_common_password(password: &str) -> bool {
    const COMMON: &str = include_str!("common_passwords.txt");
    let normalized = password.trim().to_ascii_lowercase();
    COMMON
        .lines()
        .any(|candidate| candidate.trim().eq_ignore_ascii_case(&normalized))
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
    fn embedded_common_password_list_is_complete() {
        assert!(include_str!("common_passwords.txt").lines().count() >= 1000);
    }

    #[test]
    fn rejects_short_and_common() {
        assert!(validate_password("short").is_err());
        assert!(validate_password("Password1").is_err());
        assert!(validate_password("a much longer passphrase").is_ok());
    }
}
