use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::{FromRow, SqlitePool};

use crate::{
    auth::AuthUser,
    error::{AppError, AppResult},
    models::{business_role, Role, Transaction, UserSummary},
    state::AppState,
};

#[derive(Deserialize)]
pub struct CreateTransaction {
    #[serde(rename = "type")]
    kind: String,
    amount: f64,
    note: Option<String>,
}

#[derive(FromRow)]
struct TxRow {
    id: i64,
    cash_book_id: i64,
    kind: String,
    amount_cents: i64,
    note: Option<String>,
    created_at: String,
    user_id: i64,
    name: String,
    email: String,
    phone: Option<String>,
}

impl From<TxRow> for Transaction {
    fn from(r: TxRow) -> Self {
        Transaction {
            id: r.id,
            cash_book_id: r.cash_book_id,
            kind: r.kind,
            amount: r.amount_cents as f64 / 100.0,
            note: r.note,
            created_at: r.created_at,
            user: UserSummary {
                id: r.user_id,
                name: r.name,
                email: r.email,
                phone: r.phone,
            },
        }
    }
}

const TX_SELECT: &str = "
    SELECT t.id, t.cash_book_id, t.type AS kind, t.amount_cents, t.note, t.created_at,
           u.id AS user_id, u.name, u.email, u.phone
    FROM transactions t JOIN users u ON u.id = t.user_id";

/// Resolves a cash book to its business and checks the caller can access it.
async fn authorize_book(db: &SqlitePool, cash_book_id: i64, user_id: i64) -> AppResult<Role> {
    let business_id: i64 = sqlx::query_scalar("SELECT business_id FROM cash_books WHERE id = ?")
        .bind(cash_book_id)
        .fetch_optional(db)
        .await?
        .ok_or(AppError::NotFound)?;
    business_role(db, business_id, user_id).await
}

async fn touch_book(db: &SqlitePool, cash_book_id: i64) -> AppResult<()> {
    sqlx::query(
        "UPDATE cash_books SET updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?",
    )
    .bind(cash_book_id)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(cash_book_id): Path<i64>,
) -> AppResult<Json<Value>> {
    authorize_book(&state.db, cash_book_id, auth.id).await?;
    let rows: Vec<TxRow> = sqlx::query_as(&format!(
        "{TX_SELECT} WHERE t.cash_book_id = ? ORDER BY t.created_at DESC, t.id DESC"
    ))
    .bind(cash_book_id)
    .fetch_all(&state.db)
    .await?;

    let (mut cash_in, mut cash_out) = (0i64, 0i64);
    for r in &rows {
        if r.kind == "cash_in" {
            cash_in += r.amount_cents;
        } else {
            cash_out += r.amount_cents;
        }
    }
    let transactions: Vec<Transaction> = rows.into_iter().map(Into::into).collect();
    Ok(Json(json!({
        "transactions": transactions,
        "cash_in": cash_in as f64 / 100.0,
        "cash_out": cash_out as f64 / 100.0,
        "balance": (cash_in - cash_out) as f64 / 100.0,
    })))
}

pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(cash_book_id): Path<i64>,
    Json(req): Json<CreateTransaction>,
) -> AppResult<(StatusCode, Json<Transaction>)> {
    authorize_book(&state.db, cash_book_id, auth.id).await?;
    if req.kind != "cash_in" && req.kind != "cash_out" {
        return Err(AppError::Validation(
            "Type must be 'cash_in' or 'cash_out'.".into(),
        ));
    }
    if !req.amount.is_finite() || req.amount <= 0.0 || req.amount > 1e12 {
        return Err(AppError::Validation(
            "Amount must be a positive number.".into(),
        ));
    }
    let amount_cents = (req.amount * 100.0).round() as i64;
    if amount_cents == 0 {
        return Err(AppError::Validation("Amount must be at least 0.01.".into()));
    }
    let note = req
        .note
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty());

    let id = sqlx::query(
        "INSERT INTO transactions (cash_book_id, user_id, type, amount_cents, note) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(cash_book_id)
    .bind(auth.id)
    .bind(&req.kind)
    .bind(amount_cents)
    .bind(note)
    .execute(&state.db)
    .await?
    .last_insert_rowid();
    touch_book(&state.db, cash_book_id).await?;

    let row: TxRow = sqlx::query_as(&format!("{TX_SELECT} WHERE t.id = ?"))
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    Ok((StatusCode::CREATED, Json(row.into())))
}

pub async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((cash_book_id, transaction_id)): Path<(i64, i64)>,
) -> AppResult<StatusCode> {
    let role = authorize_book(&state.db, cash_book_id, auth.id).await?;
    let author: i64 =
        sqlx::query_scalar("SELECT user_id FROM transactions WHERE id = ? AND cash_book_id = ?")
            .bind(transaction_id)
            .bind(cash_book_id)
            .fetch_optional(&state.db)
            .await?
            .ok_or(AppError::NotFound)?;
    // Members may only delete entries they created; partners and owners may delete any.
    if role < Role::Partner && author != auth.id {
        return Err(AppError::Forbidden(
            "You can only delete transactions you created.".into(),
        ));
    }
    sqlx::query("DELETE FROM transactions WHERE id = ?")
        .bind(transaction_id)
        .execute(&state.db)
        .await?;
    touch_book(&state.db, cash_book_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
