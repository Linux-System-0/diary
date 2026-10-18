use std::{fs, path::PathBuf};

use diary::{diary_file, format::{DiaryEntry, DiaryPayload, DiaryTag}};
use serde_json::{Value, json};

fn main() -> anyhow::Result<()> {
    let password = "correct horse battery staple";
    let salt_hex = "000102030405060708090a0b0c0d0e0f";
    let nonce_hex = "101112131415161718191a1b";

    let mut salt = [0u8; diary_file::SALT_LEN];
    hex::decode_to_slice(salt_hex, &mut salt)?;
    let mut nonce = [0u8; diary_file::NONCE_LEN];
    hex::decode_to_slice(nonce_hex, &mut nonce)?;

    let payload = DiaryPayload {
        format: "diary".into(),
        version: 1,
        exported_at: "2025-03-01T12:00:00Z".into(),
        app: "diary/0.1.0".into(),
        timezone: "Asia/Shanghai".into(),
        tags: vec![DiaryTag {
            name: "开心".into(),
            color: Some("#f59e0b".into()),
            emoji: Some("😄".into()),
        }],
        entries: vec![DiaryEntry {
            date: "2025-02-14".into(),
            content_md: "# 今天\n\n写了一下午代码。".into(),
            tags: vec!["开心".into()],
            created_at: Some("2025-02-14T22:10:00Z".into()),
            updated_at: Some("2025-02-14T22:30:00Z".into()),
        }],
    };
    let plaintext = serde_json::to_vec(&payload)?;
    let plaintext_text = String::from_utf8(plaintext.clone())?;

    let file = diary_file::encrypt(password, &plaintext, salt, nonce)?;
    let key = diary_file::derive_key(
        password,
        &salt,
        diary_file::ARGON2_M_COST,
        diary_file::ARGON2_T_COST,
        diary_file::ARGON2_P_COST,
    )?;
    let header_hex = hex::encode(&file[..diary_file::HEADER_LEN]);
    let ciphertext_hex = hex::encode(&file[diary_file::HEADER_LEN..]);
    let file_hex = hex::encode(&file);

    let mut bad_ciphertext = file.clone();
    let last = bad_ciphertext.len() - 1;
    bad_ciphertext[last] ^= 1;

    let mut bad_version = file.clone();
    bad_version[8] = 2;

    let mut bad_flags = file.clone();
    bad_flags[10] = 0;

    let mut bad_m_cost = file.clone();
    bad_m_cost[12..16].copy_from_slice(&1u32.to_le_bytes());

    let truncated = &file[..file.len() - 1];

    let plain = b"{\"format\":\"diary\",\"version\":1,\"tags\":[],\"entries\":[]}";
    let plain_file = diary_file::seal_plaintext(plain);

    let document = json!({
        "description": ".diary v1 cross-implementation test vectors",
        "format": "diary-test-vectors",
        "version": 1,
        "kdf": {
            "algorithm": "argon2id",
            "m_cost": diary_file::ARGON2_M_COST,
            "t_cost": diary_file::ARGON2_T_COST,
            "p_cost": diary_file::ARGON2_P_COST,
            "hash_len": 32
        },
        "vectors": [
            {
                "name": "encrypted-json-zh-emoji",
                "kind": "encrypted",
                "expect": "ok",
                "password": password,
                "salt_hex": salt_hex,
                "nonce_hex": nonce_hex,
                "plaintext_utf8": plaintext_text,
                "key_hex": hex::encode(key.as_ref()),
                "header_hex": header_hex,
                "ciphertext_hex": ciphertext_hex,
                "file_hex": file_hex
            },
            {
                "name": "decrypt-roundtrip",
                "kind": "decrypt",
                "expect": "ok",
                "password": password,
                "file_hex": file_hex,
                "plaintext_utf8": plaintext_text
            },
            {
                "name": "wrong-password",
                "kind": "decrypt",
                "expect": "fail",
                "password": "wrong password",
                "file_hex": file_hex
            },
            {
                "name": "tampered-ciphertext",
                "kind": "decrypt",
                "expect": "fail",
                "password": password,
                "file_hex": hex::encode(&bad_ciphertext)
            },
            {
                "name": "tampered-version",
                "kind": "decrypt",
                "expect": "fail",
                "password": password,
                "file_hex": hex::encode(&bad_version)
            },
            {
                "name": "tampered-flags",
                "kind": "decrypt",
                "expect": "fail",
                "password": password,
                "file_hex": hex::encode(&bad_flags)
            },
            {
                "name": "tampered-m-cost",
                "kind": "decrypt",
                "expect": "fail",
                "password": password,
                "file_hex": hex::encode(&bad_m_cost)
            },
            {
                "name": "truncated-ciphertext",
                "kind": "decrypt",
                "expect": "fail",
                "password": password,
                "file_hex": hex::encode(truncated)
            },
            {
                "name": "plaintext",
                "kind": "plaintext",
                "expect": "ok",
                "plaintext_utf8": String::from_utf8(plain.to_vec())?,
                "file_hex": hex::encode(&plain_file)
            }
        ]
    });

    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../docs/test-vectors.json");
    fs::write(&path, serde_json::to_string_pretty(&document)? + "\n")?;
    println!("wrote {}", path.display());
    // Touch the type so accidental schema changes fail at compile time.
    let _: &Value = &document;
    Ok(())
}
