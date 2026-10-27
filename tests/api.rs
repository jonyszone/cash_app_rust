use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use cash_app_backend::{
    connect, routes,
    state::{AppState, Config},
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

async fn app_with(config: Config) -> Router {
    let db = connect("sqlite::memory:").await.unwrap();
    routes::router(AppState { db, config })
}

/// App with the development mock checkout enabled and default free-plan limits.
async fn app() -> Router {
    let defaults = Config::from_env();
    app_with(Config {
        allow_mock_checkout: true,
        stripe_secret_key: None,
        stripe_webhook_secret: None,
        stripe_price_id: None,
        free_max_businesses: 1,
        free_max_cash_books: 2,
        free_max_team_members: 2,
        ..defaults
    })
    .await
}

async fn call(
    app: &Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(t) = token {
        req = req.header("authorization", format!("Bearer {t}"));
    }
    let body = body
        .map(|b| Body::from(b.to_string()))
        .unwrap_or_else(Body::empty);
    let res = app.clone().oneshot(req.body(body).unwrap()).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn register(app: &Router, name: &str, email: &str) -> String {
    let (s, b) = call(app, "POST", "/api/register", None,
        Some(json!({"name": name, "email": email, "password": "secret1", "password_confirmation": "secret1", "phone": "123"}))).await;
    assert_eq!(s, StatusCode::CREATED, "{b}");
    b["token"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn full_flow() {
    let app = app().await;
    let alice = register(&app, "Alice", "alice@example.com").await;
    let bob = register(&app, "Bob", "bob@example.com").await;

    let (s, me) = call(&app, "GET", "/api/me", Some(&alice), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(me["is_pro"], false);
    assert_eq!(
        call(&app, "GET", "/api/me", None, None).await.0,
        StatusCode::UNAUTHORIZED
    );

    let (s, login) = call(
        &app,
        "POST",
        "/api/login",
        None,
        Some(json!({"email": "ALICE@example.com", "password": "secret1"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert!(login["token"].is_string());
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/login",
            None,
            Some(json!({"email": "alice@example.com", "password": "nope"}))
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    // Business + plan limit
    let (s, biz) = call(
        &app,
        "POST",
        "/api/businesses",
        Some(&alice),
        Some(json!({"name": "Shop"})),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
    let biz_id = biz["id"].as_i64().unwrap();
    let (s, err) = call(
        &app,
        "POST",
        "/api/businesses",
        Some(&alice),
        Some(json!({"name": "Shop 2"})),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert_eq!(err["error"], "plan_limit");

    let (s, upd) = call(
        &app,
        "PUT",
        &format!("/api/businesses/{biz_id}"),
        Some(&alice),
        Some(json!({"address": "1 Main St", "type": "Retail"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(upd["address"], "1 Main St");
    assert_eq!(upd["type"], "Retail");

    // Bob can't see Alice's business
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/businesses/{biz_id}/cash-books"),
            Some(&bob),
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );

    // Cash book + transactions
    let (s, book) = call(
        &app,
        "POST",
        &format!("/api/businesses/{biz_id}/cash-books"),
        Some(&alice),
        Some(json!({"title": "March"})),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
    let book_id = book["id"].as_i64().unwrap();
    call(
        &app,
        "POST",
        &format!("/api/cash-books/{book_id}/transactions"),
        Some(&alice),
        Some(json!({"type": "cash_in", "amount": 100.5, "note": "sale"})),
    )
    .await;
    let (s, t) = call(
        &app,
        "POST",
        &format!("/api/cash-books/{book_id}/transactions"),
        Some(&alice),
        Some(json!({"type": "cash_out", "amount": 20.25})),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!(t["user"]["name"], "Alice");
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/cash-books/{book_id}/transactions"),
            Some(&alice),
            Some(json!({"type": "cash_in", "amount": -1}))
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    let (_, list) = call(
        &app,
        "GET",
        &format!("/api/cash-books/{book_id}/transactions"),
        Some(&alice),
        None,
    )
    .await;
    assert_eq!(list["cash_in"], 100.5);
    assert_eq!(list["cash_out"], 20.25);
    assert_eq!(list["balance"], 80.25);
    assert_eq!(list["transactions"].as_array().unwrap().len(), 2);

    // Team: add Bob as member; he sees the business under "member"
    let (s, member) = call(
        &app,
        "POST",
        &format!("/api/businesses/{biz_id}/team-members"),
        Some(&alice),
        Some(json!({"email": "bob@example.com", "role": "member"})),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!(member["user"]["email"], "bob@example.com");
    let (_, bizs) = call(&app, "GET", "/api/businesses", Some(&bob), None).await;
    assert_eq!(bizs["owned"].as_array().unwrap().len(), 0);
    assert_eq!(bizs["member"][0]["cash_books"][0]["balance"], 80.25);
    assert_eq!(bizs["member"][0]["cash_books"][0]["members_count"], 2);

    // Members can't delete others' transactions or create books
    let tx_id = t["id"].as_i64().unwrap();
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/cash-books/{book_id}/transactions/{tx_id}"),
            Some(&bob),
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/businesses/{biz_id}/cash-books"),
            Some(&bob),
            Some(json!({"title": "x"}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/cash-books/{book_id}/transactions/{tx_id}"),
            Some(&alice),
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );

    let (_, team) = call(
        &app,
        "GET",
        &format!("/api/businesses/{biz_id}/team-members"),
        Some(&bob),
        None,
    )
    .await;
    assert_eq!(team[0]["role"], "owner");
    assert_eq!(team.as_array().unwrap().len(), 2);

    // Upgrade via checkout lifts the business limit
    let (s, co) = call(&app, "POST", "/api/checkout", Some(&alice), None).await;
    assert_eq!(s, StatusCode::OK);
    let url = co["checkout_url"].as_str().unwrap();
    let path = &url[url.find("/api/").unwrap()..];
    let res = app
        .clone()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        call(&app, "GET", "/api/me", Some(&alice), None).await.1["is_pro"],
        true
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/businesses",
            Some(&alice),
            Some(json!({"name": "Shop 2"}))
        )
        .await
        .0,
        StatusCode::CREATED
    );

    // Logout revokes the token
    assert_eq!(
        call(&app, "POST", "/api/logout", Some(&alice), None)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "GET", "/api/me", Some(&alice), None).await.0,
        StatusCode::UNAUTHORIZED
    );
}

fn signed(secret: &str, payload: &str) -> String {
    use hmac::{Hmac, Mac};
    let t = chrono::Utc::now().timestamp();
    let mut mac = Hmac::<sha2::Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(format!("{t}.{payload}").as_bytes());
    format!("t={t},v1={}", hex::encode(mac.finalize().into_bytes()))
}

async fn webhook(app: &Router, signature: &str, payload: &str) -> StatusCode {
    let req = Request::post("/api/webhooks/stripe")
        .header("stripe-signature", signature)
        .body(Body::from(payload.to_string()))
        .unwrap();
    app.clone().oneshot(req).await.unwrap().status()
}

#[tokio::test]
async fn checkout_is_unavailable_until_payments_are_configured() {
    let app = app_with(Config {
        allow_mock_checkout: false,
        stripe_secret_key: None,
        stripe_webhook_secret: None,
        stripe_price_id: None,
        ..Config::from_env()
    })
    .await;
    let token = register(&app, "Al", "al@example.com").await;
    let (s, _) = call(&app, "POST", "/api/checkout", Some(&token), None).await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
    // No webhook secret configured: nothing can grant Pro.
    assert_eq!(
        webhook(&app, "t=1,v1=00", "{}").await,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        call(&app, "GET", "/api/me", Some(&token), None).await.1["is_pro"],
        false
    );
}

#[tokio::test]
async fn stripe_webhooks_grant_and_revoke_pro() {
    const SECRET: &str = "whsec_test";
    let app = app_with(Config {
        allow_mock_checkout: false,
        stripe_webhook_secret: Some(SECRET.into()),
        ..Config::from_env()
    })
    .await;
    let token = register(&app, "Al", "al@example.com").await;
    let user_id = call(&app, "GET", "/api/me", Some(&token), None).await.1["id"]
        .as_i64()
        .unwrap();

    // Unsigned and wrongly signed events are rejected and change nothing.
    let paid = json!({"type": "checkout.session.completed", "data": {"object": {
        "client_reference_id": user_id.to_string(), "customer": "cus_1", "subscription": "sub_1"}}})
    .to_string();
    assert_eq!(
        webhook(&app, "", &paid).await,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        webhook(&app, &signed("whsec_other", &paid), &paid).await,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(&app, "GET", "/api/me", Some(&token), None).await.1["is_pro"],
        false
    );

    // A signed payment event grants Pro.
    assert_eq!(
        webhook(&app, &signed(SECRET, &paid), &paid).await,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "GET", "/api/me", Some(&token), None).await.1["is_pro"],
        true
    );

    // Past-due subscription revokes it; active restores it; deletion revokes it.
    let update = |status: &str| {
        json!({"type": "customer.subscription.updated",
        "data": {"object": {"id": "sub_1", "status": status}}})
        .to_string()
    };
    for (status, expected) in [("past_due", false), ("active", true)] {
        let body = update(status);
        assert_eq!(
            webhook(&app, &signed(SECRET, &body), &body).await,
            StatusCode::OK
        );
        assert_eq!(
            call(&app, "GET", "/api/me", Some(&token), None).await.1["is_pro"],
            expected
        );
    }
    let deleted = json!({"type": "customer.subscription.deleted",
        "data": {"object": {"id": "sub_1", "status": "canceled"}}})
    .to_string();
    assert_eq!(
        webhook(&app, &signed(SECRET, &deleted), &deleted).await,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "GET", "/api/me", Some(&token), None).await.1["is_pro"],
        false
    );
}
