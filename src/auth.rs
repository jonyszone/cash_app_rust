use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{async_trait, extract::FromRequestParts, http::request::Parts};
use rand::{distributions::Alphanumeric, Rng};

use crate::{
    error::{AppError, AppResult},
    state::AppState,
};

pub fn hash_password(password: &str) -> AppResult<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AppError::Internal(e.to_string()))
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .map(|parsed| {
            Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok()
        })
        .unwrap_or(false)
}

pub fn random_token(len: usize) -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

/// The authenticated caller, resolved from an `Authorization: Bearer <token>` header.
pub struct AuthUser {
    pub id: i64,
    pub token: String,
}

#[async_trait]
impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> AppResult<Self> {
        let token = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or(AppError::Unauthorized)?
            .trim()
            .to_string();

        let user_id: i64 = sqlx::query_scalar("SELECT user_id FROM tokens WHERE token = ?")
            .bind(&token)
            .fetch_optional(&state.db)
            .await?
            .ok_or(AppError::Unauthorized)?;

        Ok(AuthUser { id: user_id, token })
    }
}
