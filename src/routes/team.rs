use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use sqlx::{FromRow, SqlitePool};

use crate::{
    auth::AuthUser,
    error::{AppError, AppResult},
    models::{business_role, find_user, require_role, Role, TeamMember, UserSummary},
    state::AppState,
};

#[derive(Deserialize)]
pub struct AddMember {
    email: String,
    role: String,
}

#[derive(FromRow)]
struct MemberRow {
    id: i64,
    business_id: i64,
    role: String,
    created_at: String,
    user_id: i64,
    name: String,
    email: String,
    phone: Option<String>,
}

impl From<MemberRow> for TeamMember {
    fn from(r: MemberRow) -> Self {
        TeamMember {
            id: r.id,
            business_id: r.business_id,
            role: r.role,
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

const MEMBER_SELECT: &str = "
    SELECT tm.id, tm.business_id, tm.role, tm.created_at,
           u.id AS user_id, u.name, u.email, u.phone
    FROM team_members tm JOIN users u ON u.id = tm.user_id";

async fn load_member(db: &SqlitePool, id: i64) -> AppResult<TeamMember> {
    let row: MemberRow = sqlx::query_as(&format!("{MEMBER_SELECT} WHERE tm.id = ?"))
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(row.into())
}

pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(business_id): Path<i64>,
) -> AppResult<Json<Vec<TeamMember>>> {
    business_role(&state.db, business_id, auth.id).await?;
    let rows: Vec<MemberRow> = sqlx::query_as(&format!(
        "{MEMBER_SELECT} WHERE tm.business_id = ?
         ORDER BY CASE tm.role WHEN 'owner' THEN 0 WHEN 'partner' THEN 1 ELSE 2 END, tm.id"
    ))
    .bind(business_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows.into_iter().map(Into::into).collect()))
}

pub async fn add(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(business_id): Path<i64>,
    Json(req): Json<AddMember>,
) -> AppResult<(StatusCode, Json<TeamMember>)> {
    let caller = require_role(&state.db, business_id, auth.id, Role::Partner).await?;
    let role = match Role::parse(&req.role) {
        Some(r @ (Role::Partner | Role::Member)) => r,
        _ => {
            return Err(AppError::Validation(
                "Role must be either 'partner' or 'member'.".into(),
            ))
        }
    };
    if role == Role::Partner && caller != Role::Owner {
        return Err(AppError::Forbidden(
            "Only the owner can add partners.".into(),
        ));
    }

    let user_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(req.email.trim().to_lowercase())
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::Validation("No registered user found with that email.".into()))?;

    let owner_id: i64 = sqlx::query_scalar("SELECT owner_id FROM businesses WHERE id = ?")
        .bind(business_id)
        .fetch_one(&state.db)
        .await?;
    if !find_user(&state.db, owner_id).await?.is_pro {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM team_members WHERE business_id = ? AND role != 'owner'",
        )
        .bind(business_id)
        .fetch_one(&state.db)
        .await?;
        if count >= state.config.free_max_team_members {
            return Err(AppError::PlanLimit(format!(
                "Free plan allows {} team members per business. Upgrade to Pro to add more.",
                state.config.free_max_team_members
            )));
        }
    }

    let role_str = if role == Role::Partner {
        "partner"
    } else {
        "member"
    };
    let id = sqlx::query(
        "INSERT INTO team_members (business_id, user_id, role) VALUES (?, ?, ?)
         ON CONFLICT (business_id, user_id) DO NOTHING",
    )
    .bind(business_id)
    .bind(user_id)
    .bind(role_str)
    .execute(&state.db)
    .await?;
    if id.rows_affected() == 0 {
        return Err(AppError::Conflict(
            "That user is already a member of this business.".into(),
        ));
    }
    Ok((
        StatusCode::CREATED,
        Json(load_member(&state.db, id.last_insert_rowid()).await?),
    ))
}

pub async fn remove(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((business_id, member_id)): Path<(i64, i64)>,
) -> AppResult<StatusCode> {
    let caller = business_role(&state.db, business_id, auth.id).await?;
    let target = load_member(&state.db, member_id).await?;
    if target.business_id != business_id {
        return Err(AppError::NotFound);
    }
    let target_role = Role::parse(&target.role).unwrap_or(Role::Member);
    if target_role == Role::Owner {
        return Err(AppError::Forbidden("The owner cannot be removed.".into()));
    }
    // Anyone may leave; otherwise you must outrank the person you remove.
    let leaving = target.user.id == auth.id;
    if !leaving && (caller < Role::Partner || caller <= target_role) {
        return Err(AppError::Forbidden(
            "You do not have permission to remove this member.".into(),
        ));
    }
    sqlx::query("DELETE FROM team_members WHERE id = ?")
        .bind(member_id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
