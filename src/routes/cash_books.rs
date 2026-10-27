use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use sqlx::SqlitePool;

use crate::{
    auth::AuthUser,
    error::{AppError, AppResult},
    models::{business_role, find_user, require_role, CashBook, Role, CASH_BOOK_SELECT},
    state::AppState,
};

#[derive(Deserialize)]
pub struct CreateCashBook {
    title: String,
}

pub async fn load_cash_book(db: &SqlitePool, id: i64) -> AppResult<CashBook> {
    sqlx::query_as(&format!("{CASH_BOOK_SELECT} WHERE cb.id = ?"))
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or(AppError::NotFound)
}

pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(business_id): Path<i64>,
) -> AppResult<Json<Vec<CashBook>>> {
    business_role(&state.db, business_id, auth.id).await?;
    let books = sqlx::query_as(&format!(
        "{CASH_BOOK_SELECT} WHERE cb.business_id = ? ORDER BY cb.updated_at DESC, cb.id DESC"
    ))
    .bind(business_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(books))
}

pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(business_id): Path<i64>,
    Json(req): Json<CreateCashBook>,
) -> AppResult<(StatusCode, Json<CashBook>)> {
    require_role(&state.db, business_id, auth.id, Role::Partner).await?;
    let title = req.title.trim();
    if title.is_empty() {
        return Err(AppError::Validation("Cash book title is required.".into()));
    }

    // Limits follow the business owner's plan, so partners can't bypass them.
    let owner_id: i64 = sqlx::query_scalar("SELECT owner_id FROM businesses WHERE id = ?")
        .bind(business_id)
        .fetch_one(&state.db)
        .await?;
    if !find_user(&state.db, owner_id).await?.is_pro {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cash_books WHERE business_id = ?")
            .bind(business_id)
            .fetch_one(&state.db)
            .await?;
        if count >= state.config.free_max_cash_books {
            return Err(AppError::PlanLimit(format!(
                "Free plan allows {} cash books per business. Upgrade to Pro for unlimited cash books.",
                state.config.free_max_cash_books
            )));
        }
    }

    let id = sqlx::query("INSERT INTO cash_books (business_id, title) VALUES (?, ?)")
        .bind(business_id)
        .bind(title)
        .execute(&state.db)
        .await?
        .last_insert_rowid();
    Ok((StatusCode::CREATED, Json(load_cash_book(&state.db, id).await?)))
}

pub async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((business_id, cash_book_id)): Path<(i64, i64)>,
) -> AppResult<StatusCode> {
    require_role(&state.db, business_id, auth.id, Role::Partner).await?;
    let result = sqlx::query("DELETE FROM cash_books WHERE id = ? AND business_id = ?")
        .bind(cash_book_id)
        .bind(business_id)
        .execute(&state.db)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}
