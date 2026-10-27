use sqlx::SqlitePool;

#[derive(Clone)]
pub struct Config {
    /// Public base URL of this server, used to build checkout links.
    pub public_url: String,
    pub free_max_businesses: i64,
    pub free_max_cash_books: i64,
    pub free_max_team_members: i64,
    /// Stripe secret key (`sk_...`). When unset, real checkout is unavailable.
    pub stripe_secret_key: Option<String>,
    /// Signing secret for the webhook endpoint (`whsec_...`).
    pub stripe_webhook_secret: Option<String>,
    /// Price id (`price_...`) of the recurring Pro plan.
    pub stripe_price_id: Option<String>,
    /// Development only: let `/checkout` grant Pro without taking payment.
    pub allow_mock_checkout: bool,
}

impl Config {
    pub fn from_env() -> Self {
        let num = |key: &str, default: i64| {
            std::env::var(key)
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default)
        };
        let opt = |key: &str| std::env::var(key).ok().filter(|v| !v.is_empty());
        Self {
            stripe_secret_key: opt("STRIPE_SECRET_KEY"),
            stripe_webhook_secret: opt("STRIPE_WEBHOOK_SECRET"),
            stripe_price_id: opt("STRIPE_PRICE_ID"),
            allow_mock_checkout: opt("ALLOW_MOCK_CHECKOUT").is_some_and(|v| v == "true"),
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
