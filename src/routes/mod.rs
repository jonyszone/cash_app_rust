use axum::{
    routing::{delete, get, post, put},
    Router,
};

use crate::state::AppState;

mod auth;
mod billing;
mod businesses;
mod cash_books;
mod team;
mod transactions;

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/register", post(auth::register))
        .route("/login", post(auth::login))
        .route("/logout", post(auth::logout))
        .route("/me", get(auth::me).put(auth::update_me))
        .route(
            "/businesses",
            get(businesses::list).post(businesses::create),
        )
        .route(
            "/businesses/:id",
            put(businesses::update).delete(businesses::delete),
        )
        .route(
            "/businesses/:id/cash-books",
            get(cash_books::list).post(cash_books::create),
        )
        .route(
            "/businesses/:id/cash-books/:book_id",
            delete(cash_books::delete),
        )
        .route(
            "/businesses/:id/team-members",
            get(team::list).post(team::add),
        )
        .route(
            "/businesses/:id/team-members/:member_id",
            delete(team::remove),
        )
        .route(
            "/cash-books/:id/transactions",
            get(transactions::list).post(transactions::create),
        )
        .route(
            "/cash-books/:id/transactions/:tx_id",
            delete(transactions::delete),
        )
        .route("/checkout", post(billing::checkout))
        .route("/checkout/:session/complete", get(billing::complete));

    Router::new()
        .route("/health", get(|| async { "ok" }))
        .nest("/api", api)
        .with_state(state)
}
