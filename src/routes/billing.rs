//! Pro subscription billing via Stripe Checkout.
//!
//! `POST /checkout` creates a Stripe Checkout Session and returns its URL; the
//! app opens it in the browser. Pro is granted and revoked only by Stripe's
//! signed webhooks (`POST /webhooks/stripe`), never by the client.
//!
//! For local development without Stripe, set `ALLOW_MOCK_CHECKOUT=true`: the
//! returned URL then upgrades the user to Pro when opened, with no payment.

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::Html,
    Json,
};
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::Sha256;

use crate::{
    auth::{random_token, AuthUser},
    error::{AppError, AppResult},
    models::find_user,
    state::{AppState, Config},
};

const STRIPE_API: &str = "https://api.stripe.com/v1";
/// Webhooks older than this are rejected, to limit replay.
const SIGNATURE_TOLERANCE_SECS: i64 = 300;

pub async fn checkout(State(state): State<AppState>, auth: AuthUser) -> AppResult<Json<Value>> {
    let user = find_user(&state.db, auth.id).await?;
    if user.is_pro {
        return Err(AppError::Conflict(
            "You are already on the Pro plan.".into(),
        ));
    }

    let cfg = &state.config;
    if let (Some(key), Some(price)) = (&cfg.stripe_secret_key, &cfg.stripe_price_id) {
        let customer: Option<String> =
            sqlx::query_scalar("SELECT stripe_customer_id FROM users WHERE id = ?")
                .bind(auth.id)
                .fetch_one(&state.db)
                .await?;
        let params = checkout_params(cfg, price, auth.id, &user.email, customer.as_deref());
        let res = reqwest::Client::new()
            .post(format!("{STRIPE_API}/checkout/sessions"))
            .bearer_auth(key)
            .form(&params)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("stripe request failed: {e}")))?;
        if !res.status().is_success() {
            let body = res.text().await.unwrap_or_default();
            return Err(AppError::Internal(format!(
                "stripe rejected checkout: {body}"
            )));
        }
        let session: Value = res
            .json()
            .await
            .map_err(|e| AppError::Internal(format!("bad stripe response: {e}")))?;
        let url = session["url"]
            .as_str()
            .ok_or_else(|| AppError::Internal("stripe response had no url".into()))?;
        return Ok(Json(json!({ "checkout_url": url })));
    }

    if !cfg.allow_mock_checkout {
        return Err(AppError::Unavailable("Payments are not configured.".into()));
    }
    let session_id = random_token(40);
    sqlx::query("INSERT INTO checkout_sessions (id, user_id) VALUES (?, ?)")
        .bind(&session_id)
        .bind(auth.id)
        .execute(&state.db)
        .await?;
    let url = format!(
        "{}/api/checkout/{}/complete",
        cfg.public_url.trim_end_matches('/'),
        session_id
    );
    Ok(Json(json!({ "checkout_url": url })))
}

/// Form fields for `POST /v1/checkout/sessions`.
fn checkout_params(
    cfg: &Config,
    price_id: &str,
    user_id: i64,
    email: &str,
    customer: Option<&str>,
) -> Vec<(String, String)> {
    let base = cfg.public_url.trim_end_matches('/');
    let mut p = vec![
        ("mode".to_string(), "subscription".to_string()),
        ("line_items[0][price]".into(), price_id.to_string()),
        ("line_items[0][quantity]".into(), "1".into()),
        ("client_reference_id".into(), user_id.to_string()),
        (
            "subscription_data[metadata][user_id]".into(),
            user_id.to_string(),
        ),
        ("success_url".into(), format!("{base}/api/checkout/success")),
        ("cancel_url".into(), format!("{base}/api/checkout/cancel")),
    ];
    // Reuse the Stripe customer if we have one; otherwise let Checkout create it.
    match customer {
        Some(c) => p.push(("customer".into(), c.to_string())),
        None => p.push(("customer_email".into(), email.to_string())),
    }
    p
}

pub async fn success() -> Html<&'static str> {
    Html(
        "<html><body style=\"font-family:sans-serif;text-align:center;padding:3em\">\
         <h1>Thank you!</h1><p>Your Pro plan will be active in a moment. \
         You can return to the app now.</p></body></html>",
    )
}

pub async fn cancel() -> Html<&'static str> {
    Html(
        "<html><body style=\"font-family:sans-serif;text-align:center;padding:3em\">\
         <h1>Checkout cancelled</h1><p>You can return to the app.</p></body></html>",
    )
}

/// Development-only stand-in for completing payment (see module docs).
pub async fn complete(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> AppResult<Html<&'static str>> {
    if !state.config.allow_mock_checkout {
        return Err(AppError::NotFound);
    }
    let mut tx = state.db.begin().await?;
    let user_id: i64 = sqlx::query_scalar(
        "UPDATE checkout_sessions SET completed = 1 WHERE id = ? AND completed = 0 RETURNING user_id",
    )
    .bind(&session_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AppError::NotFound)?;
    sqlx::query("UPDATE users SET is_pro = 1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Html(
        "<html><body style=\"font-family:sans-serif;text-align:center;padding:3em\">\
         <h1>You're on Pro!</h1><p>You can return to the app now.</p></body></html>",
    ))
}

/// Checks a `Stripe-Signature` header (`t=<unix>,v1=<hex hmac>[,v1=...]`)
/// against the raw request body.
pub fn verify_stripe_signature(secret: &str, header: &str, payload: &[u8], now_unix: i64) -> bool {
    let mut timestamp = None;
    let mut signatures = Vec::new();
    for part in header.split(',') {
        match part.trim().split_once('=') {
            Some(("t", v)) => timestamp = v.parse::<i64>().ok(),
            Some(("v1", v)) => signatures.push(v),
            _ => {}
        }
    }
    let Some(t) = timestamp else { return false };
    if (now_unix - t).abs() > SIGNATURE_TOLERANCE_SECS {
        return false;
    }
    signatures.iter().any(|sig| {
        let Ok(expected) = hex::decode(sig) else {
            return false;
        };
        let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret.as_bytes()) else {
            return false;
        };
        mac.update(t.to_string().as_bytes());
        mac.update(b".");
        mac.update(payload);
        // verify_slice compares in constant time.
        mac.verify_slice(&expected).is_ok()
    })
}

pub async fn stripe_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> AppResult<StatusCode> {
    let secret = state
        .config
        .stripe_webhook_secret
        .as_deref()
        .ok_or_else(|| AppError::Unavailable("Webhooks are not configured.".into()))?;
    let signature = headers
        .get("stripe-signature")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    let now = chrono::Utc::now().timestamp();
    if !verify_stripe_signature(secret, signature, &body, now) {
        return Err(AppError::Validation("Invalid signature.".into()));
    }

    let event: Value = serde_json::from_slice(&body)
        .map_err(|_| AppError::Validation("Invalid payload.".into()))?;
    let object = &event["data"]["object"];
    match event["type"].as_str().unwrap_or_default() {
        // Payment succeeded: link the Stripe customer and subscription, grant Pro.
        "checkout.session.completed" => {
            let user_id = object["client_reference_id"]
                .as_str()
                .and_then(|s| s.parse::<i64>().ok());
            if let Some(user_id) = user_id {
                sqlx::query(
                    "UPDATE users SET is_pro = 1, stripe_customer_id = ?, stripe_subscription_id = ?,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?",
                )
                .bind(object["customer"].as_str())
                .bind(object["subscription"].as_str())
                .bind(user_id)
                .execute(&state.db)
                .await?;
            }
        }
        // Subscription changed: Pro only while it is active or trialing.
        "customer.subscription.updated" | "customer.subscription.deleted" => {
            let active = event["type"] == "customer.subscription.updated"
                && matches!(object["status"].as_str(), Some("active" | "trialing"));
            if let Some(sub_id) = object["id"].as_str() {
                sqlx::query(
                    "UPDATE users SET is_pro = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
                     WHERE stripe_subscription_id = ?",
                )
                .bind(active)
                .bind(sub_id)
                .execute(&state.db)
                .await?;
            }
        }
        _ => {}
    }
    Ok(StatusCode::OK)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sign(secret: &str, t: i64, payload: &[u8]) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(format!("{t}.").as_bytes());
        mac.update(payload);
        format!("t={t},v1={}", hex::encode(mac.finalize().into_bytes()))
    }

    #[test]
    fn accepts_valid_signature() {
        let header = sign("whsec_x", 1000, b"{}");
        assert!(verify_stripe_signature("whsec_x", &header, b"{}", 1000));
    }

    #[test]
    fn rejects_tampered_wrong_secret_stale_and_malformed() {
        let header = sign("whsec_x", 1000, b"{}");
        assert!(!verify_stripe_signature(
            "whsec_x",
            &header,
            b"{\"a\":1}",
            1000
        ));
        assert!(!verify_stripe_signature("whsec_y", &header, b"{}", 1000));
        assert!(!verify_stripe_signature(
            "whsec_x",
            &header,
            b"{}",
            1000 + 301
        ));
        assert!(!verify_stripe_signature("whsec_x", "garbage", b"{}", 1000));
        assert!(!verify_stripe_signature(
            "whsec_x",
            "t=1000,v1=zz",
            b"{}",
            1000
        ));
    }

    #[test]
    fn checkout_params_reuse_customer_or_email() {
        let cfg = Config::from_env();
        let new = checkout_params(&cfg, "price_1", 7, "a@b.c", None);
        assert!(new.contains(&("customer_email".into(), "a@b.c".into())));
        assert!(new.contains(&("client_reference_id".into(), "7".into())));
        let existing = checkout_params(&cfg, "price_1", 7, "a@b.c", Some("cus_9"));
        assert!(existing.contains(&("customer".into(), "cus_9".into())));
        assert!(!existing.iter().any(|(k, _)| k == "customer_email"));
    }
}
