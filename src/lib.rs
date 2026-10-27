pub mod auth;
pub mod error;
pub mod models;
pub mod routes;
pub mod state;

use std::str::FromStr;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};

pub type BoxResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Opens (creating if needed) the SQLite database and runs migrations.
pub async fn connect(database_url: &str) -> BoxResult<SqlitePool> {
    let options = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .foreign_keys(true);
    // Each connection to an in-memory database is a separate database.
    let max = if database_url.contains(":memory:") {
        1
    } else {
        10
    };
    let pool = SqlitePoolOptions::new()
        .max_connections(max)
        .connect_with(options)
        .await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}
