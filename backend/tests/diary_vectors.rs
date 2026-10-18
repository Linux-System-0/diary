use std::fs;

use diary::diary_file;
use serde_json::Value;

fn vectors() -> Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/test-vectors.json");
    serde_json::from_str(&fs::read_to_string(path).expect("读取测试向量")).expect("解析测试向量")
}

fn hex_bytes(value: &Value, key: &str) -> Vec<u8> {
    hex::decode(value[key].as_str().expect("十六进制字段")).expect("合法十六进制")
}

#[test]
fn rust_reads_shared_test_vectors() {
    let document = vectors();
    let list = document["vectors"].as_array().expect("vectors 数组");
    assert!(!list.is_empty());

    for vector in list {
        let name = vector["name"].as_str().expect("name");
        let expect_ok = vector["expect"] == "ok";
        let kind = vector["kind"].as_str().expect("kind");

        match kind {
            "encrypted" => {
                let password = vector["password"].as_str().expect("password");
                let plaintext = vector["plaintext_utf8"]
                    .as_str()
                    .expect("plaintext")
                    .as_bytes();
                let mut salt = [0u8; diary_file::SALT_LEN];
                salt.copy_from_slice(&hex_bytes(vector, "salt_hex"));
                let mut nonce = [0u8; diary_file::NONCE_LEN];
                nonce.copy_from_slice(&hex_bytes(vector, "nonce_hex"));

                let file = diary_file::encrypt(password, plaintext, salt, nonce).expect("加密");
                assert_eq!(hex::encode(&file), vector["file_hex"].as_str().unwrap(), "{name}");
                assert_eq!(
                    hex::encode(&file[..diary_file::HEADER_LEN]),
                    vector["header_hex"].as_str().unwrap(),
                    "{name}"
                );
                assert_eq!(
                    hex::encode(&file[diary_file::HEADER_LEN..]),
                    vector["ciphertext_hex"].as_str().unwrap(),
                    "{name}"
                );

                let key = diary_file::derive_key(
                    password,
                    &salt,
                    diary_file::ARGON2_M_COST,
                    diary_file::ARGON2_T_COST,
                    diary_file::ARGON2_P_COST,
                )
                .expect("派生 key");
                assert_eq!(hex::encode(key.as_ref()), vector["key_hex"].as_str().unwrap(), "{name}");
            }
            "decrypt" | "plaintext" => {
                let password = vector["password"].as_str().unwrap_or("");
                let file = hex_bytes(vector, "file_hex");
                let result = diary_file::decrypt(password, &file, u64::MAX);
                assert_eq!(result.is_ok(), expect_ok, "{name}: {result:?}");
                if expect_ok {
                    assert_eq!(
                        String::from_utf8(result.unwrap()).unwrap(),
                        vector["plaintext_utf8"].as_str().unwrap(),
                        "{name}"
                    );
                }
            }
            other => panic!("未知向量类型 {other}"),
        }
    }
}
