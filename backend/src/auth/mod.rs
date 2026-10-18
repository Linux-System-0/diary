mod cookies;
mod crypto;
mod extract;
mod jwt;
mod password;
mod session;
mod totp;

pub use cookies::{REFRESH_COOKIE, clear_refresh_cookie, read_refresh_cookie, refresh_cookie};
pub use crypto::{decrypt_secret, encrypt_secret};
pub use extract::{AuthUser, UserRow, apply_twofa_disable_deadline, load_user};
pub use session::{issue_refresh, revoke_all_for_user, revoke_refresh, rotate_refresh};
pub use totp::{
    build as build_totp, check_now as check_totp_now, generate_secret as generate_totp_secret,
    normalize_code as normalize_totp_code, otpauth_uri as totp_uri,
    secret_base32 as totp_secret_base32,
};

pub use jwt::{Decoded, IssuedToken, decode_access, decode_mfa, issue_access, issue_mfa};
pub use password::{hash_password, password_needs_rehash, validate_password, verify_password};
