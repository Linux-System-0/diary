use std::time::Duration;

use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    error::{ApiError, ApiResult},
    util::now_unix,
};

const ISSUER: &str = "diary";
const AUD_ACCESS: &str = "diary-web";
const AUD_MFA: &str = "diary-mfa";
const CLOCK_LEEWAY_SECS: u64 = 60;

#[derive(Debug, Serialize, Deserialize)]
struct AccessClaims {
    sub: String,
    typ: String,
    ver: i64,
    jti: String,
    iat: i64,
    exp: i64,
    iss: String,
    aud: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct MfaClaims {
    sub: String,
    typ: String,
    scope: String,
    jti: String,
    iat: i64,
    exp: i64,
    iss: String,
    aud: String,
}

#[derive(Debug, Deserialize)]
struct RawClaims {
    sub: String,
    typ: String,
    #[serde(default)]
    ver: Option<i64>,
    #[serde(default)]
    scope: Option<String>,
    jti: String,
    #[serde(default)]
    iat: Option<i64>,
    exp: i64,
    iss: String,
    aud: String,
}

pub struct IssuedToken {
    pub token: String,
    pub expires_in: u64,
}

#[derive(Debug)]
pub enum Decoded {
    Access {
        user_id: i64,
        token_version: i64,
        jti: String,
    },
    Mfa {
        user_id: i64,
    },
}

pub fn issue_access(
    secret: &[u8],
    user_id: i64,
    token_version: i64,
    ttl: Duration,
) -> ApiResult<IssuedToken> {
    let now = now_unix();
    let expires_in = ttl.as_secs();
    let claims = AccessClaims {
        sub: user_id.to_string(),
        typ: "access".into(),
        ver: token_version,
        jti: Uuid::new_v4().to_string(),
        iat: now,
        exp: now + expires_in as i64,
        iss: ISSUER.into(),
        aud: AUD_ACCESS.into(),
    };
    Ok(IssuedToken {
        token: encode(&claims, secret)?,
        expires_in,
    })
}

pub fn issue_mfa(secret: &[u8], user_id: i64, ttl: Duration) -> ApiResult<IssuedToken> {
    let now = now_unix();
    let expires_in = ttl.as_secs();
    let claims = MfaClaims {
        sub: user_id.to_string(),
        typ: "mfa".into(),
        scope: "mfa".into(),
        jti: Uuid::new_v4().to_string(),
        iat: now,
        exp: now + expires_in as i64,
        iss: ISSUER.into(),
        aud: AUD_MFA.into(),
    };
    Ok(IssuedToken {
        token: encode(&claims, secret)?,
        expires_in,
    })
}

pub fn decode_access(secret: &[u8], token: &str) -> ApiResult<Decoded> {
    match decode_any(secret, token)? {
        access @ Decoded::Access { .. } => Ok(access),
        Decoded::Mfa { .. } => Err(ApiError::unauthorized("mfa_required", "请先完成两步验证")),
    }
}

pub fn decode_mfa(secret: &[u8], token: &str) -> ApiResult<i64> {
    match decode_any(secret, token)? {
        Decoded::Mfa { user_id } => Ok(user_id),
        Decoded::Access { .. } => Err(ApiError::unauthorized(
            "invalid_token",
            "登录状态无效或已过期",
        )),
    }
}

fn decode_any(secret: &[u8], token: &str) -> ApiResult<Decoded> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.leeway = CLOCK_LEEWAY_SECS;
    validation.validate_aud = false;
    validation.set_issuer(&[ISSUER]);
    validation.set_required_spec_claims(&["exp", "sub", "iss", "aud"]);

    let data =
        jsonwebtoken::decode::<RawClaims>(token, &DecodingKey::from_secret(secret), &validation)
            .map_err(|_| ApiError::unauthorized("invalid_token", "登录状态无效或已过期"))?;
    let claims = data.claims;

    if claims.iss != ISSUER {
        return Err(ApiError::unauthorized(
            "invalid_token",
            "登录状态无效或已过期",
        ));
    }

    let user_id = claims
        .sub
        .parse::<i64>()
        .map_err(|_| ApiError::unauthorized("invalid_token", "登录状态无效或已过期"))?;
    if claims.exp <= now_unix() - CLOCK_LEEWAY_SECS as i64 {
        return Err(ApiError::unauthorized(
            "invalid_token",
            "登录状态无效或已过期",
        ));
    }
    if claims
        .iat
        .is_some_and(|iat| iat > now_unix() + CLOCK_LEEWAY_SECS as i64)
    {
        return Err(ApiError::unauthorized(
            "invalid_token",
            "登录状态无效或已过期",
        ));
    }

    match (claims.typ.as_str(), claims.aud.as_str()) {
        ("access", AUD_ACCESS) => {
            let ver = claims
                .ver
                .ok_or_else(|| ApiError::unauthorized("invalid_token", "登录状态无效或已过期"))?;
            Ok(Decoded::Access {
                user_id,
                token_version: ver,
                jti: claims.jti,
            })
        }
        ("mfa", AUD_MFA) if claims.scope.as_deref() == Some("mfa") => Ok(Decoded::Mfa { user_id }),
        _ => Err(ApiError::unauthorized(
            "invalid_token",
            "登录状态无效或已过期",
        )),
    }
}

fn encode<T: Serialize>(claims: &T, secret: &[u8]) -> ApiResult<String> {
    jsonwebtoken::encode(
        &Header::new(Algorithm::HS256),
        claims,
        &EncodingKey::from_secret(secret),
    )
    .map_err(|error| ApiError::internal(anyhow::anyhow!("JWT 签发失败: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"0123456789abcdef0123456789abcdef";

    #[test]
    fn access_and_mfa_are_not_interchangeable() {
        let access = issue_access(SECRET, 42, 3, Duration::from_secs(60)).unwrap();
        let mfa = issue_mfa(SECRET, 42, Duration::from_secs(60)).unwrap();

        assert!(matches!(
            decode_access(SECRET, &access.token).unwrap(),
            Decoded::Access {
                token_version: 3,
                ..
            }
        ));
        assert_eq!(decode_mfa(SECRET, &mfa.token).unwrap(), 42);

        let error = decode_access(SECRET, &mfa.token).unwrap_err();
        assert_eq!(error.code(), "mfa_required");
        assert_eq!(
            decode_mfa(SECRET, &access.token).unwrap_err().code(),
            "invalid_token"
        );
    }

    #[test]
    fn tampered_token_is_rejected() {
        let access = issue_access(SECRET, 1, 0, Duration::from_secs(60)).unwrap();
        let mut tampered = access.token.into_bytes();
        let last = tampered.len() - 1;
        tampered[last] = if tampered[last] == b'a' { b'b' } else { b'a' };
        let tampered = String::from_utf8(tampered).unwrap();
        assert_eq!(
            decode_access(SECRET, &tampered).unwrap_err().code(),
            "invalid_token"
        );
    }

    #[test]
    fn wrong_secret_is_rejected() {
        let access = issue_access(SECRET, 1, 0, Duration::from_secs(60)).unwrap();
        assert!(decode_access(b"another-secret-another-secret-32", &access.token).is_err());
    }
}
