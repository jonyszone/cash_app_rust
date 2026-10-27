# Cash App Backend

Rust (axum + SQLite) REST API for the [cash_app_flutter](https://github.com/jonyszone/cash_app_flutter) client.

## Run

```sh
cargo run            # listens on 0.0.0.0:8000, creates ./cash_app.db
cargo test
```

The Flutter app's default `baseUrl` (`http://10.0.2.2:8000/api`) reaches this from the Android emulator.

### Configuration (env vars or `.env`)

| Variable | Default | |
|---|---|---|
| `DATABASE_URL` | `sqlite://cash_app.db` | |
| `BIND_ADDR` | `0.0.0.0:8000` | |
| `PUBLIC_URL` | `http://localhost:8000` | Used to build checkout links |
| `FREE_MAX_BUSINESSES` | `1` | Free-plan limits; exceeding one returns `403 {"error":"plan_limit"}`, which opens the app's paywall |
| `FREE_MAX_CASH_BOOKS` | `2` | per business |
| `FREE_MAX_TEAM_MEMBERS` | `2` | per business, excluding the owner |

## API (all under `/api`, bearer-token auth except register/login)

| Method | Path | Notes |
|---|---|---|
| POST | `/register` | `{name, email, password, password_confirmation, phone}` → `{token, user}` |
| POST | `/login` | `{email, password}` → `{token, user}` |
| POST | `/logout` | revokes the current token |
| GET/PUT | `/me` | user incl. `is_pro`; PUT accepts `{name, phone}` |
| GET | `/businesses` | `{owned: [...], member: [...]}`, each with `cash_books` |
| POST | `/businesses` | `{name}` |
| PUT/DELETE | `/businesses/:id` | `name, address, staff_size, category, type, email, whatsapp` |
| GET/POST | `/businesses/:id/cash-books` | `{title}`; books include `balance`, `members_count` |
| DELETE | `/businesses/:id/cash-books/:book_id` | |
| GET/POST | `/businesses/:id/team-members` | `{email, role: partner\|member}` |
| DELETE | `/businesses/:id/team-members/:member_id` | |
| GET/POST | `/cash-books/:id/transactions` | `{type: cash_in\|cash_out, amount, note?}`; GET → `{transactions, cash_in, cash_out, balance}` |
| DELETE | `/cash-books/:id/transactions/:tx_id` | |
| POST | `/checkout` | `{checkout_url}` |

Errors are `{"error": code, "message": text}`.

### Roles
- **owner** – everything, including deleting the business and adding partners.
- **partner** – edit business, manage cash books, add/remove members, delete any transaction.
- **member** – view and add transactions; delete only their own.

### Payments
`/checkout` currently uses a built-in mock: the returned URL upgrades the user to Pro when opened. Replace `src/routes/billing.rs::complete` with a real provider webhook (e.g. Stripe) before production.

Amounts are stored as integer cents.
