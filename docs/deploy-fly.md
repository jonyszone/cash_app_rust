# Deploying to Fly.io

This runs the backend as a single Fly machine with its SQLite database on a Fly volume. Fly terminates HTTPS for you, which the Flutter app's release builds require.

You need a Fly.io account (with a payment method on file) and a Stripe account for payments.

## 1. Install and sign in

```sh
curl -L https://fly.io/install.sh | sh     # or: brew install flyctl
fly auth login
```

## 2. Create the app

`fly.toml` is already in the repo. Pick a globally unique app name and a [region](https://fly.io/docs/reference/regions/) near your users, then edit these three lines in `fly.toml`:

```toml
app = "your-app-name"
primary_region = "iad"
...
  PUBLIC_URL = "https://your-app-name.fly.dev"
```

Create the app without deploying yet:

```sh
fly apps create your-app-name
```

## 3. Create the data volume

The database lives here. Use the same region as `primary_region`.

```sh
fly volumes create cash_app_data --region iad --size 1
```

Fly keeps daily snapshots of volumes for a few days, but see [Backups](#backups) below.

## 4. Set secrets

Secrets are stored encrypted and are not in `fly.toml` or git.

```sh
fly secrets set \
  STRIPE_SECRET_KEY=sk_test_... \
  STRIPE_PRICE_ID=price_... \
  STRIPE_WEBHOOK_SECRET=whsec_placeholder
```

Use Stripe **test-mode** keys first. You don't have the real webhook secret yet (step 6), so the placeholder is replaced later.

> Do **not** set `ALLOW_MOCK_CHECKOUT`. It lets anyone get Pro without paying and is for local development only.

Optional free-plan limits can be set the same way: `FREE_MAX_BUSINESSES`, `FREE_MAX_CASH_BOOKS`, `FREE_MAX_TEAM_MEMBERS`.

## 5. Deploy

```sh
fly deploy --ha=false
```

`--ha=false` keeps it to **one** machine. A Fly volume attaches to a single machine, and SQLite can't be shared between machines, so don't scale this out (see [Scaling](#scaling)).

The first build compiles the Rust code on Fly's builder and takes a few minutes. Check it:

```sh
curl https://your-app-name.fly.dev/health      # -> ok
fly status
fly logs
```

## 6. Connect Stripe

1. In the Stripe dashboard (test mode), go to **Developers → Webhooks → Add endpoint**.
2. Endpoint URL: `https://your-app-name.fly.dev/api/webhooks/stripe`
3. Events: `checkout.session.completed`, `customer.subscription.updated`, `customer.subscription.deleted`.
4. Copy the endpoint's **signing secret** (`whsec_...`) and set it:

```sh
fly secrets set STRIPE_WEBHOOK_SECRET=whsec_...
```

Setting a secret restarts the machine automatically.

## 7. Test a payment

1. Point the app at your server (step 8) and register a user.
2. Trigger an upgrade, for example by hitting a free-plan limit and tapping the upgrade button.
3. Pay with Stripe's test card `4242 4242 4242 4242` (any future expiry, any CVC).
4. Check `GET /api/me` (or the app): `is_pro` should become `true` within a few seconds. If not, look at the webhook's delivery log in Stripe and at `fly logs`.
5. Cancel the subscription in the Stripe dashboard and confirm `is_pro` goes back to `false`.

When it all works, switch to your **live-mode** Stripe keys, price and webhook secret with `fly secrets set`.

## 8. Point the Flutter app at the server

Build the app with the server's address:

```sh
flutter build apk --release --dart-define=API_BASE_URL=https://your-app-name.fly.dev/api
```

(or `flutter run --dart-define=API_BASE_URL=...` while testing).

## Day-to-day

| Task | Command |
|---|---|
| Deploy a new version | `fly deploy --ha=false` |
| Watch logs | `fly logs` |
| Machine status | `fly status` |
| Change a setting | `fly secrets set NAME=value` |
| Shell into the machine | `fly ssh console` |

## Backups

Fly takes automatic daily volume snapshots, but you should also keep your own copy of the data. `fly ssh console` has no `sqlite3` binary, so download the file instead:

```sh
fly ssh sftp get /data/cash_app.db ./cash_app-backup.db
```

A copy taken while the app is writing can be slightly inconsistent; for a clean copy, run it at a quiet time, and test restoring it before you rely on it. To restore from a snapshot, see `fly volumes snapshots list` and `fly volumes create --snapshot-id`.

## Scaling

One shared-CPU machine comfortably serves a small app. To grow:

- **Bigger machine:** `fly scale vm shared-cpu-2x` / `fly scale memory 1024`.
- **More than one machine** needs a database that can be shared between machines (for example Postgres). That is a code change, since the backend currently only supports SQLite.

## Costs

You pay for the machine, the volume and outbound traffic. Check Fly's [pricing page](https://fly.io/docs/about/pricing/) for current numbers.
