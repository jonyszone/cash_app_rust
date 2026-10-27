//! Pro subscription checkout.
//!
//! This is a self-contained stand-in for a payment provider: `POST /checkout`
//! returns a one-time URL, and opening it upgrades the user to Pro. Swap
//! `complete` for a real provider's webhook (e.g. Stripe) before going live.

use axum::{
    extract::{Path, State},
    response::Html,
    Json,
};
use serde_json::{json, Value};

use crate::{
    auth::{random_token, AuthUser},
    error::{AppError, AppResult},
    models::find_user,
    state::AppState,
};

pub async fn checkout(State(state): State<AppState>, auth: AuthUser) -> AppResult<Json<Value>> {
    if find_user(&state.db, auth.id).await?.is_pro {
        return Err(AppError::Conflict("You are already on the Pro plan.".into()));
    }
    let session_id = random_token(40);
    sqlx::query("INSERT INTO checkout_sessions (id, user_id) VALUES (?, ?)")
        .bind(&session_id)
        .bind(auth.id)
        .execute(&state.db)
        .await?;
    let url = format!(
        "{}/api/checkout/{}/complete",
        state.config.public_url.trim_end_matches('/'),
        session_id
    );
    Ok(Json(json!({ "checkout_url": url })))
}

pub async fn complete(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> AppResult<Html<&'static str>> {
    let mut tx = state.db.begin().await?;
    let user_id: i64 = sqlx::query_scalar(
        "UPDATE checkout_sessions SET completed = 1 WHERE id = ? AND completed = 0 RETURNING user_id",
    )
    .bind(&session_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AppError::NotFound)?;
    sqlx::query("UPDATE users SET is_pro = 1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Html(
        "<html><body style=\"font-family:sans-serif;text-align:center;padding:3em\">\
         <h1>You're on Pro!</h1><p>You can return to the app now.</p></body></html>",
    ))
}
