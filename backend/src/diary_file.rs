use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use anyhow::{Result, bail};
use argon2::{Algorithm, Argon2, Params, Version};
use zeroize::Zeroizing;

pub const MAGIC: [u8; 8] = *b"DIARY\x1a\x01\x00";
pub const VERSION: u16 = 1;
pub const HEADER_LEN: usize = 57;
pub const FLAG_ENCRYPTED: u8 = 0b0000_0001;
pub const KDF_NONE: u8 = 0;
pub const KDF_ARGON2ID: u8 = 1;
pub const SALT_LEN: usize = 16;
pub const NONCE_LEN: usize = 12;
pub const TAG_LEN: usize = 16;
pub const MAX_M_COST: u32 = 262_144;
pub const MAX_T_COST: u32 = 10;

pub const ARGON2_M_COST: u32 = 65_536;
pub const ARGON2_T_COST: u32 = 3;
pub const ARGON2_P_COST: u8 = 1;

#[derive(Debug, Clone)]
pub struct Header {
    pub version: u16,
    pub flags: u8,
    pub kdf: u8,
    pub m_cost: u32,
    pub t_cost: u32,
    pub p_cost: u8,
    pub salt: [u8; SALT_LEN],
    pub nonce: [u8; NONCE_LEN],
    pub payload_len: u64,
}

impl Header {
    pub fn encrypted() -> Self {
        Self {
            version: VERSION,
            flags: FLAG_ENCRYPTED,
            kdf: KDF_ARGON2ID,
            m_cost: ARGON2_M_COST,
            t_cost: ARGON2_T_COST,
            p_cost: ARGON2_P_COST,
            salt: [0; SALT_LEN],
            nonce: [0; NONCE_LEN],
            payload_len: 0,
        }
    }

    pub fn plaintext(payload_len: u64) -> Self {
        Self {
            version: VERSION,
            flags: 0,
            kdf: KDF_NONE,
            m_cost: 0,
            t_cost: 0,
            p_cost: 0,
            salt: [0; SALT_LEN],
            nonce: [0; NONCE_LEN],
            payload_len,
        }
    }

    pub fn to_bytes(&self) -> [u8; HEADER_LEN] {
        let mut out = [0u8; HEADER_LEN];
        out[0..8].copy_from_slice(&MAGIC);
        out[8..10].copy_from_slice(&self.version.to_le_bytes());
        out[10] = self.flags;
        out[11] = self.kdf;
        out[12..16].copy_from_slice(&self.m_cost.to_le_bytes());
        out[16..20].copy_from_slice(&self.t_cost.to_le_bytes());
        out[20] = self.p_cost;
        out[21..37].copy_from_slice(&self.salt);
        out[37..49].copy_from_slice(&self.nonce);
        out[49..57].copy_from_slice(&self.payload_len.to_le_bytes());
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < HEADER_LEN {
            bail!("文件头不完整");
        }
        if bytes[0..8] != MAGIC {
            bail!("magic 不匹配，不是 .diary 文件");
        }
        let version = u16::from_le_bytes([bytes[8], bytes[9]]);
        if version != VERSION {
            bail!("不支持的 .diary 版本 {version}");
        }
        let flags = bytes[10];
        if flags & !FLAG_ENCRYPTED != 0 {
            bail!("flags 含未知位");
        }
        let kdf = bytes[11];
        if kdf != KDF_NONE && kdf != KDF_ARGON2ID {
            bail!("未知 KDF");
        }
        let m_cost = u32::from_le_bytes(bytes[12..16].try_into().expect("4 字节"));
        let t_cost = u32::from_le_bytes(bytes[16..20].try_into().expect("4 字节"));
        let p_cost = bytes[20];
        let mut salt = [0u8; SALT_LEN];
        salt.copy_from_slice(&bytes[21..37]);
        let mut nonce = [0u8; NONCE_LEN];
        nonce.copy_from_slice(&bytes[37..49]);
        let payload_len = u64::from_le_bytes(bytes[49..57].try_into().expect("8 字节"));

        let encrypted = flags & FLAG_ENCRYPTED != 0;
        match kdf {
            KDF_NONE => {
                if encrypted {
                    bail!("未加密文件不能带 ENCRYPTED 标志");
                }
                if m_cost != 0 || t_cost != 0 || p_cost != 0 {
                    bail!("KDF=none 时参数必须为 0");
                }
                if salt.iter().any(|byte| *byte != 0) || nonce.iter().any(|byte| *byte != 0) {
                    bail!("KDF=none 时 salt/nonce 必须为 0");
                }
            }
            KDF_ARGON2ID => {
                if !encrypted {
                    bail!("KDF=argon2id 时必须设置 ENCRYPTED 标志");
                }
                if m_cost == 0 || t_cost == 0 || p_cost == 0 {
                    bail!("Argon2id 参数不能为 0");
                }
                if m_cost > MAX_M_COST || t_cost > MAX_T_COST {
                    bail!("KDF 参数超过上限");
                }
            }
            _ => unreachable!(),
        }

        Ok(Self {
            version,
            flags,
            kdf,
            m_cost,
            t_cost,
            p_cost,
            salt,
            nonce,
            payload_len,
        })
    }
}

pub fn encrypt(
    password: &str,
    plaintext: &[u8],
    salt: [u8; SALT_LEN],
    nonce: [u8; NONCE_LEN],
) -> Result<Vec<u8>> {
    encrypt_with_params(
        password,
        plaintext,
        salt,
        nonce,
        ARGON2_M_COST,
        ARGON2_T_COST,
        ARGON2_P_COST,
    )
}

pub fn encrypt_with_params(
    password: &str,
    plaintext: &[u8],
    salt: [u8; SALT_LEN],
    nonce: [u8; NONCE_LEN],
    m_cost: u32,
    t_cost: u32,
    p_cost: u8,
) -> Result<Vec<u8>> {
    let key = derive_key(password, &salt, m_cost, t_cost, p_cost)?;
    let cipher =
        Aes256Gcm::new_from_slice(key.as_ref()).map_err(|_| anyhow::anyhow!("AES 密钥长度错误"))?;

    let mut header = Header::encrypted();
    header.m_cost = m_cost;
    header.t_cost = t_cost;
    header.p_cost = p_cost;
    header.salt = salt;
    header.nonce = nonce;

    // Encrypt with a zero length first so the header length is known, then fill
    // it in. GCM ciphertext length equals plaintext length, so a second pass is
    // unnecessary: header.payload_len = plaintext.len() + tag.
    header.payload_len = (plaintext.len() + TAG_LEN) as u64;
    let aad = header.to_bytes();
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad: &aad,
            },
        )
        .map_err(|_| anyhow::anyhow!("AES-GCM 加密失败"))?;

    let mut file = Vec::with_capacity(HEADER_LEN + ciphertext.len());
    file.extend_from_slice(&aad);
    file.extend_from_slice(&ciphertext);
    Ok(file)
}

pub fn seal_plaintext(plaintext: &[u8]) -> Vec<u8> {
    let header = Header::plaintext(plaintext.len() as u64);
    let mut file = Vec::with_capacity(HEADER_LEN + plaintext.len());
    file.extend_from_slice(&header.to_bytes());
    file.extend_from_slice(plaintext);
    file
}

pub fn decrypt(password: &str, file: &[u8], max_payload: u64) -> Result<Vec<u8>> {
    let header = Header::from_bytes(file)?;
    if header.payload_len > max_payload {
        bail!("payload 超过上限");
    }
    if header.payload_len != (file.len() - HEADER_LEN) as u64 {
        bail!("payload_len 与实际长度不一致");
    }

    let payload = &file[HEADER_LEN..];
    if header.kdf == KDF_NONE {
        return Ok(payload.to_vec());
    }
    if payload.len() < TAG_LEN {
        bail!("密文被截断");
    }

    let key = derive_key(
        password,
        &header.salt,
        header.m_cost,
        header.t_cost,
        header.p_cost,
    )?;
    let cipher =
        Aes256Gcm::new_from_slice(key.as_ref()).map_err(|_| anyhow::anyhow!("AES 密钥长度错误"))?;
    let aad = &file[..HEADER_LEN];
    cipher
        .decrypt(
            Nonce::from_slice(&header.nonce),
            Payload { msg: payload, aad },
        )
        .map_err(|_| anyhow::anyhow!("密码错误或文件已损坏"))
}

pub fn derive_key(
    password: &str,
    salt: &[u8; SALT_LEN],
    m_cost: u32,
    t_cost: u32,
    p_cost: u8,
) -> Result<Zeroizing<[u8; 32]>> {
    let params = Params::new(m_cost, t_cost, p_cost as u32, Some(32))
        .map_err(|error| anyhow::anyhow!("Argon2 参数非法: {error}"))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = Zeroizing::new([0u8; 32]);
    argon2
        .hash_password_into(password.as_bytes(), salt, key.as_mut())
        .map_err(|error| anyhow::anyhow!("Argon2id 派生失败: {error}"))?;
    Ok(key)
}

pub fn parse(file: &[u8]) -> Result<Header> {
    Header::from_bytes(file)
}

// Compile-time layout checks: the header is part of the on-disk format.
const _: () = assert!(HEADER_LEN == 57);
const _: () = assert!(MAGIC.len() == 8);

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> (String, Vec<u8>, [u8; 16], [u8; 12]) {
        (
            "correct horse battery staple".into(),
            "# 今天

写了一下午代码。😄
"
            .as_bytes()
            .to_vec(),
            [7u8; SALT_LEN],
            [9u8; NONCE_LEN],
        )
    }

    #[test]
    fn encrypted_roundtrip() {
        let (password, plaintext, salt, nonce) = sample();
        let file = encrypt(&password, &plaintext, salt, nonce).unwrap();
        assert_eq!(
            Header::from_bytes(&file).unwrap().payload_len as usize,
            plaintext.len() + TAG_LEN
        );
        assert_eq!(decrypt(&password, &file, u64::MAX).unwrap(), plaintext);
        assert!(decrypt("wrong password", &file, u64::MAX).is_err());
    }

    #[test]
    fn plaintext_roundtrip() {
        let (_, plaintext, _, _) = sample();
        let file = seal_plaintext(&plaintext);
        assert_eq!(decrypt("", &file, u64::MAX).unwrap(), plaintext);
    }

    #[test]
    fn tampering_is_rejected() {
        let (password, plaintext, salt, nonce) = sample();
        let file = encrypt(&password, &plaintext, salt, nonce).unwrap();

        let mut bad_ciphertext = file.clone();
        let last = bad_ciphertext.len() - 1;
        bad_ciphertext[last] ^= 1;
        assert!(decrypt(&password, &bad_ciphertext, u64::MAX).is_err());

        let mut bad_version = file.clone();
        bad_version[8] = 2;
        assert!(decrypt(&password, &bad_version, u64::MAX).is_err());

        let mut bad_flags = file.clone();
        bad_flags[10] = 0;
        assert!(decrypt(&password, &bad_flags, u64::MAX).is_err());

        let mut bad_m_cost = file.clone();
        bad_m_cost[12..16].copy_from_slice(&1u32.to_le_bytes());
        assert!(decrypt(&password, &bad_m_cost, u64::MAX).is_err());

        let truncated = &file[..file.len() - 1];
        assert!(decrypt(&password, truncated, u64::MAX).is_err());
    }

    #[test]
    fn payload_limit_is_enforced() {
        let (password, plaintext, salt, nonce) = sample();
        let file = encrypt(&password, &plaintext, salt, nonce).unwrap();
        assert!(decrypt(&password, &file, 4).is_err());
    }

    #[test]
    fn malicious_kdf_params_are_rejected() {
        let mut header = Header::encrypted().to_bytes();
        header[12..16].copy_from_slice(&(MAX_M_COST + 1).to_le_bytes());
        assert!(Header::from_bytes(&header).is_err());

        let mut header = Header::encrypted().to_bytes();
        header[16..20].copy_from_slice(&(MAX_T_COST + 1).to_le_bytes());
        assert!(Header::from_bytes(&header).is_err());
    }
}
