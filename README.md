# Cash App Backend

Rust (axum + SQLite) REST API for the [cash_app_flutter](https://github.com/jonyszone/cash_app_flutter) client.

## Run

```sh
cargo run            # listens on 0.0.0.0:8000, creates ./cash_app.db
cargo test
```

The Flutter app's default `baseUrl` (`http://10.0.2.2:8000/api`) reaches this from the Android emulator.

### Docker

```sh
docker build -t cash-app-backend .
docker run -d -p 8000:8000 -v cash-app-data:/data \
  -e PUBLIC_URL=https://api.example.com \
  -e STRIPE_SECRET_KEY=sk_... -e STRIPE_PRICE_ID=price_... -e STRIPE_WEBHOOK_SECRET=whsec_... \
  cash-app-backend
```

The SQLite database lives in the `/data` volume, so keep it mounted or data is lost when the container is replaced. The container runs as a non-root user and serves plain HTTP; put it behind a reverse proxy or load balancer that terminates HTTPS. Use `GET /health` for health checks.

### Configuration (env vars or `.env`)

| Variable | Default | |
|---|---|---|
| `DATABASE_URL` | `sqlite://cash_app.db` | |
| `BIND_ADDR` | `0.0.0.0:8000` | |
| `PUBLIC_URL` | `http://localhost:8000` | Used to build checkout links |
| `FREE_MAX_BUSINESSES` | `1` | Free-plan limits; exceeding one returns `403 {"error":"plan_limit"}`, which opens the app's paywall |
| `FREE_MAX_CASH_BOOKS` | `2` | per business |
| `FREE_MAX_TEAM_MEMBERS` | `2` | per business, excluding the owner |
| `STRIPE_SECRET_KEY` | – | `sk_...`; enables real checkout |
| `STRIPE_PRICE_ID` | – | `price_...` of the recurring Pro plan |
| `STRIPE_WEBHOOK_SECRET` | – | `whsec_...` of the webhook endpoint |
| `ALLOW_MOCK_CHECKOUT` | – | `true` for local development only: `/checkout` upgrades to Pro with no payment |

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

### Payments (Stripe)
`POST /checkout` creates a Stripe Checkout Session (subscription mode) and returns its URL, which the app opens in the browser. Pro is granted and revoked only by Stripe's signed webhooks, never by the client.

Setup:
1. In Stripe, create a recurring Price for the Pro plan and set `STRIPE_PRICE_ID` and `STRIPE_SECRET_KEY`.
2. Add a webhook endpoint at `{PUBLIC_URL}/api/webhooks/stripe` for `checkout.session.completed`, `customer.subscription.updated` and `customer.subscription.deleted`; set its signing secret as `STRIPE_WEBHOOK_SECRET`.
3. Set `PUBLIC_URL` to the server's public HTTPS address.

Pro stays on while the subscription is `active` or `trialing`. Without Stripe configured, `/checkout` returns `503` unless `ALLOW_MOCK_CHECKOUT=true` (development only).

For local testing, forward events with the Stripe CLI: `stripe listen --forward-to localhost:8000/api/webhooks/stripe`.

Amounts are stored as integer cents.
