use axum::{extract::State, http::StatusCode, Json};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    auth::{hash_password, random_token, verify_password, AuthUser},
    error::{AppError, AppResult},
    models::{find_user, User},
    state::AppState,
};

#[derive(Deserialize)]
pub struct RegisterRequest {
    name: String,
    email: String,
    password: String,
    password_confirmation: Option<String>,
    phone: Option<String>,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    email: String,
    password: String,
}

#[derive(Deserialize)]
pub struct UpdateProfileRequest {
    name: Option<String>,
    phone: Option<String>,
}

async fn issue_token(state: &AppState, user_id: i64) -> AppResult<String> {
    let token = random_token(64);
    sqlx::query("INSERT INTO tokens (token, user_id) VALUES (?, ?)")
        .bind(&token)
        .bind(user_id)
        .execute(&state.db)
        .await?;
    Ok(token)
}

pub async fn register(
    State(state): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> AppResult<(StatusCode, Json<Value>)> {
    let name = req.name.trim();
    let email = req.email.trim().to_lowercase();
    if name.is_empty() {
        return Err(AppError::Validation("Name is required.".into()));
    }
    if !email.contains('@') || email.len() < 3 {
        return Err(AppError::Validation("A valid email is required.".into()));
    }
    if req.password.len() < 6 {
        return Err(AppError::Validation(
            "Password must be at least 6 characters.".into(),
        ));
    }
    if req
        .password_confirmation
        .as_ref()
        .is_some_and(|c| c != &req.password)
    {
        return Err(AppError::Validation("Password confirmation does not match.".into()));
    }

    let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(&email)
        .fetch_optional(&state.db)
        .await?;
    if exists.is_some() {
        return Err(AppError::Validation("The email has already been taken.".into()));
    }

    let phone = req.phone.map(|p| p.trim().to_string()).filter(|p| !p.is_empty());
    let id = sqlx::query("INSERT INTO users (name, email, phone, password_hash) VALUES (?, ?, ?, ?)")
        .bind(name)
        .bind(&email)
        .bind(phone)
        .bind(hash_password(&req.password)?)
        .execute(&state.db)
        .await?
        .last_insert_rowid();

    let token = issue_token(&state, id).await?;
    let user = find_user(&state.db, id).await?;
    Ok((StatusCode::CREATED, Json(json!({ "token": token, "user": user }))))
}

pub async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> AppResult<Json<Value>> {
    let row: Option<(i64, String)> =
        sqlx::query_as("SELECT id, password_hash FROM users WHERE email = ?")
            .bind(req.email.trim().to_lowercase())
            .fetch_optional(&state.db)
            .await?;
    let id = match row {
        Some((id, hash)) if verify_password(&req.password, &hash) => id,
        _ => return Err(AppError::Validation("Invalid email or password.".into())),
    };
    let token = issue_token(&state, id).await?;
    let user = find_user(&state.db, id).await?;
    Ok(Json(json!({ "token": token, "user": user })))
}

pub async fn logout(State(state): State<AppState>, auth: AuthUser) -> AppResult<Json<Value>> {
    sqlx::query("DELETE FROM tokens WHERE token = ?")
        .bind(&auth.token)
        .execute(&state.db)
        .await?;
    Ok(Json(json!({ "message": "Logged out." })))
}

pub async fn me(State(state): State<AppState>, auth: AuthUser) -> AppResult<Json<User>> {
    Ok(Json(find_user(&state.db, auth.id).await?))
}

pub async fn update_me(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<UpdateProfileRequest>,
) -> AppResult<Json<User>> {
    if let Some(name) = req.name.as_deref().map(str::trim) {
        if name.is_empty() {
            return Err(AppError::Validation("Name cannot be empty.".into()));
        }
        sqlx::query("UPDATE users SET name = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?")
            .bind(name)
            .bind(auth.id)
            .execute(&state.db)
            .await?;
    }
    if let Some(phone) = req.phone {
        let phone = Some(phone.trim().to_string()).filter(|p| !p.is_empty());
        sqlx::query("UPDATE users SET phone = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?")
            .bind(phone)
            .bind(auth.id)
            .execute(&state.db)
            .await?;
    }
    Ok(Json(find_user(&state.db, auth.id).await?))
}
