use sqlx::SqlitePool;

#[derive(Clone)]
pub struct Config {
    /// Public base URL of this server, used to build checkout links.
    pub public_url: String,
    pub free_max_businesses: i64,
    pub free_max_cash_books: i64,
    pub free_max_team_members: i64,
}

impl Config {
    pub fn from_env() -> Self {
        let num = |key: &str, default: i64| {
            std::env::var(key)
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default)
        };
        Self {
            public_url: std::env::var("PUBLIC_URL")
                .unwrap_or_else(|_| "http://localhost:8000".into()),
            free_max_businesses: num("FREE_MAX_BUSINESSES", 1),
            free_max_cash_books: num("FREE_MAX_CASH_BOOKS", 2),
            free_max_team_members: num("FREE_MAX_TEAM_MEMBERS", 2),
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub config: Config,
}
