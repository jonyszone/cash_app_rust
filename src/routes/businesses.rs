use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::SqlitePool;

use crate::{
    auth::AuthUser,
    error::{AppError, AppResult},
    models::{find_user, require_role, Business, CashBook, Role, CASH_BOOK_SELECT},
    state::AppState,
};

#[derive(Serialize)]
pub struct BusinessWithBooks {
    #[serde(flatten)]
    business: Business,
    role: String,
    cash_books: Vec<CashBook>,
}

#[derive(Deserialize)]
pub struct CreateBusiness {
    name: String,
}

#[derive(Deserialize)]
pub struct UpdateBusiness {
    name: Option<String>,
    address: Option<String>,
    staff_size: Option<String>,
    category: Option<String>,
    #[serde(rename = "type")]
    business_type: Option<String>,
    email: Option<String>,
    whatsapp: Option<String>,
}

pub async fn load_business(db: &SqlitePool, id: i64) -> AppResult<Business> {
    sqlx::query_as("SELECT * FROM businesses WHERE id = ?")
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or(AppError::NotFound)
}

async fn with_books(
    db: &SqlitePool,
    business: Business,
    role: String,
) -> AppResult<BusinessWithBooks> {
    let cash_books = sqlx::query_as(&format!(
        "{CASH_BOOK_SELECT} WHERE cb.business_id = ? ORDER BY cb.updated_at DESC, cb.id DESC"
    ))
    .bind(business.id)
    .fetch_all(db)
    .await?;
    Ok(BusinessWithBooks {
        business,
        role,
        cash_books,
    })
}

pub async fn list(State(state): State<AppState>, auth: AuthUser) -> AppResult<Json<Value>> {
    let rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT business_id, role FROM team_members WHERE user_id = ? ORDER BY business_id",
    )
    .bind(auth.id)
    .fetch_all(&state.db)
    .await?;

    let mut owned = Vec::new();
    let mut member = Vec::new();
    for (business_id, role) in rows {
        let business = load_business(&state.db, business_id).await?;
        let is_owner = role == "owner";
        let full = with_books(&state.db, business, role).await?;
        if is_owner {
            owned.push(full);
        } else {
            member.push(full);
        }
    }
    Ok(Json(json!({ "owned": owned, "member": member })))
}

pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CreateBusiness>,
) -> AppResult<(StatusCode, Json<BusinessWithBooks>)> {
    let name = req.name.trim();
    if name.is_empty() {
        return Err(AppError::Validation("Business name is required.".into()));
    }

    let user = find_user(&state.db, auth.id).await?;
    if !user.is_pro {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM businesses WHERE owner_id = ?")
            .bind(auth.id)
            .fetch_one(&state.db)
            .await?;
        if count >= state.config.free_max_businesses {
            return Err(AppError::PlanLimit(format!(
                "Free plan allows {} business(es). Upgrade to Pro for unlimited businesses.",
                state.config.free_max_businesses
            )));
        }
    }

    let mut tx = state.db.begin().await?;
    let id = sqlx::query("INSERT INTO businesses (owner_id, name) VALUES (?, ?)")
        .bind(auth.id)
        .bind(name)
        .execute(&mut *tx)
        .await?
        .last_insert_rowid();
    sqlx::query("INSERT INTO team_members (business_id, user_id, role) VALUES (?, ?, 'owner')")
        .bind(id)
        .bind(auth.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    let business = load_business(&state.db, id).await?;
    Ok((
        StatusCode::CREATED,
        Json(with_books(&state.db, business, "owner".into()).await?),
    ))
}

pub async fn update(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
    Json(req): Json<UpdateBusiness>,
) -> AppResult<Json<BusinessWithBooks>> {
    let role = require_role(&state.db, id, auth.id, Role::Partner).await?;
    let mut business = load_business(&state.db, id).await?;

    if let Some(name) = req.name {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err(AppError::Validation(
                "Business name cannot be empty.".into(),
            ));
        }
        business.name = name;
    }
    let clean = |v: Option<String>, current: Option<String>| match v {
        Some(s) => Some(s.trim().to_string()).filter(|s| !s.is_empty()),
        None => current,
    };
    business.address = clean(req.address, business.address);
    business.staff_size = clean(req.staff_size, business.staff_size);
    business.category = clean(req.category, business.category);
    business.business_type = clean(req.business_type, business.business_type);
    business.email = clean(req.email, business.email);
    business.whatsapp = clean(req.whatsapp, business.whatsapp);

    sqlx::query(
        "UPDATE businesses SET name = ?, address = ?, staff_size = ?, category = ?, type = ?,
         email = ?, whatsapp = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?",
    )
    .bind(&business.name)
    .bind(&business.address)
    .bind(&business.staff_size)
    .bind(&business.category)
    .bind(&business.business_type)
    .bind(&business.email)
    .bind(&business.whatsapp)
    .bind(id)
    .execute(&state.db)
    .await?;

    let business = load_business(&state.db, id).await?;
    let role = match role {
        Role::Owner => "owner",
        Role::Partner => "partner",
        Role::Member => "member",
    };
    Ok(Json(with_books(&state.db, business, role.into()).await?))
}

pub async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    require_role(&state.db, id, auth.id, Role::Owner).await?;
    sqlx::query("DELETE FROM businesses WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
