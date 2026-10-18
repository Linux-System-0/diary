use anyhow::{Result, bail};
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use rand::{RngCore, rngs::OsRng};
use zeroize::Zeroizing;

const KEY_ID: u8 = 0;
const NONCE_LEN: usize = 24;
const TAG_LEN: usize = 16;

/// `key_id(1) || nonce(24) || ciphertext || tag(16)`
pub fn encrypt_secret(master_key: &[u8; 32], secret: &[u8]) -> Result<Vec<u8>> {
    let key = Zeroizing::new(*master_key);
    let cipher = XChaCha20Poly1305::new_from_slice(key.as_ref())
        .map_err(|_| anyhow::anyhow!("主密钥长度错误"))?;
    let mut nonce = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce);
    let ciphertext = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: secret,
                aad: &[KEY_ID],
            },
        )
        .map_err(|_| anyhow::anyhow!("TOTP secret 加密失败"))?;

    let mut out = Vec::with_capacity(1 + NONCE_LEN + ciphertext.len());
    out.push(KEY_ID);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

pub fn decrypt_secret(master_key: &[u8; 32], encoded: &[u8]) -> Result<Vec<u8>> {
    if encoded.len() < 1 + NONCE_LEN + TAG_LEN {
        bail!("TOTP secret 密文太短");
    }
    let (key_id, rest) = encoded.split_first().expect("已检查长度");
    if *key_id != KEY_ID {
        bail!("未知的 TOTP secret 密钥版本");
    }
    let (nonce, ciphertext) = rest.split_at(NONCE_LEN);
    let key = Zeroizing::new(*master_key);
    let cipher = XChaCha20Poly1305::new_from_slice(key.as_ref())
        .map_err(|_| anyhow::anyhow!("主密钥长度错误"))?;
    cipher
        .decrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad: &[*key_id],
            },
        )
        .map_err(|_| anyhow::anyhow!("TOTP secret 解密失败"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_tamper() {
        let key = [7u8; 32];
        let encrypted = encrypt_secret(&key, b"super-secret-totp").unwrap();
        assert_eq!(
            decrypt_secret(&key, &encrypted).unwrap(),
            b"super-secret-totp"
        );

        let mut tampered = encrypted.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        assert!(decrypt_secret(&key, &tampered).is_err());

        let mut bad_id = encrypted.clone();
        bad_id[0] = 9;
        assert!(decrypt_secret(&key, &bad_id).is_err());

        assert!(decrypt_secret(&[8u8; 32], &encrypted).is_err());
    }
}
