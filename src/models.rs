use serde::Serialize;
use sqlx::{FromRow, SqlitePool};

use crate::error::{AppError, AppResult};

#[derive(Serialize, FromRow)]
pub struct User {
    pub id: i64,
    pub name: String,
    pub email: String,
    pub phone: Option<String>,
    pub is_pro: bool,
    pub created_at: String,
}

pub const USER_COLUMNS: &str = "id, name, email, phone, is_pro, created_at";

pub async fn find_user(db: &SqlitePool, id: i64) -> AppResult<User> {
    sqlx::query_as(&format!("SELECT {USER_COLUMNS} FROM users WHERE id = ?"))
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or(AppError::NotFound)
}

#[derive(Serialize, FromRow)]
pub struct Business {
    pub id: i64,
    pub owner_id: i64,
    pub name: String,
    pub address: Option<String>,
    pub staff_size: Option<String>,
    pub category: Option<String>,
    #[serde(rename = "type")]
    #[sqlx(rename = "type")]
    pub business_type: Option<String>,
    pub email: Option<String>,
    pub whatsapp: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Serialize, FromRow)]
pub struct CashBook {
    pub id: i64,
    pub business_id: i64,
    pub title: String,
    pub balance: f64,
    pub members_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

pub const CASH_BOOK_SELECT: &str = "
    SELECT cb.id, cb.business_id, cb.title,
        COALESCE((SELECT SUM(CASE WHEN t.type = 'cash_in' THEN t.amount_cents ELSE -t.amount_cents END)
                  FROM transactions t WHERE t.cash_book_id = cb.id), 0) / 100.0 AS balance,
        (SELECT COUNT(*) FROM team_members tm WHERE tm.business_id = cb.business_id) AS members_count,
        cb.created_at, cb.updated_at
    FROM cash_books cb";

#[derive(Serialize)]
pub struct UserSummary {
    pub id: i64,
    pub name: String,
    pub email: String,
    pub phone: Option<String>,
}

#[derive(Serialize)]
pub struct TeamMember {
    pub id: i64,
    pub business_id: i64,
    pub role: String,
    pub user: UserSummary,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct Transaction {
    pub id: i64,
    pub cash_book_id: i64,
    #[serde(rename = "type")]
    pub kind: String,
    pub amount: f64,
    pub note: Option<String>,
    pub user: UserSummary,
    pub created_at: String,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Role {
    Member,
    Partner,
    Owner,
}

impl Role {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "owner" => Some(Role::Owner),
            "partner" => Some(Role::Partner),
            "member" => Some(Role::Member),
            _ => None,
        }
    }
}

/// Returns the caller's role in a business, or NotFound if they have no access
/// (so that the existence of other people's businesses is not leaked).
pub async fn business_role(db: &SqlitePool, business_id: i64, user_id: i64) -> AppResult<Role> {
    let role: Option<String> =
        sqlx::query_scalar("SELECT role FROM team_members WHERE business_id = ? AND user_id = ?")
            .bind(business_id)
            .bind(user_id)
            .fetch_optional(db)
            .await?;
    role.as_deref()
        .and_then(Role::parse)
        .ok_or(AppError::NotFound)
}

pub async fn require_role(
    db: &SqlitePool,
    business_id: i64,
    user_id: i64,
    min: Role,
) -> AppResult<Role> {
    let role = business_role(db, business_id, user_id).await?;
    if role < min {
        return Err(AppError::Forbidden(
            "You do not have permission to perform this action.".into(),
        ));
    }
    Ok(role)
}
