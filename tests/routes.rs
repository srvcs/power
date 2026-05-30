use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::post;
use axum::{Json, Router as AxumRouter};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use srvcs_power::{api::Deps, health, router, telemetry};
use tower::ServiceExt;

/// Mock `srvcs-multiply` that actually COMPUTES `a * b` from the request body.
///
/// This is deliberate: a fixed-response mock would not exercise the counted-loop
/// fold at all (every iteration would return the same number). By computing the
/// real product we verify the accumulator threads correctly through `exp`
/// sequential calls.
async fn spawn_computing_multiply() -> String {
    let app = AxumRouter::new().route(
        "/",
        post(|Json(body): Json<Value>| async move {
            let a = body.get("a").and_then(Value::as_i64).unwrap_or(0);
            let b = body.get("b").and_then(Value::as_i64).unwrap_or(0);
            (StatusCode::OK, Json(json!({ "result": a * b })))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

fn app(multiply_url: &str) -> axum::Router {
    router(
        telemetry::metrics_handle_for_tests(),
        Deps {
            multiply_url: multiply_url.to_string(),
        },
    )
}

async fn eval(multiply_url: &str, base: i64, exp: i64) -> (StatusCode, Value) {
    let res = app(multiply_url)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/")
                .header("content-type", "application/json")
                .body(Body::from(json!({ "base": base, "exp": exp }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

const DEAD_URL: &str = "http://127.0.0.1:1";

async fn status_of(uri: &str) -> StatusCode {
    app(DEAD_URL)
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn healthz_ok() {
    assert_eq!(status_of("/healthz").await, StatusCode::OK);
}

#[tokio::test]
async fn readyz_reflects_state() {
    health::set_ready(true);
    assert_eq!(status_of("/readyz").await, StatusCode::OK);
}

#[tokio::test]
async fn openapi_ok() {
    assert_eq!(status_of("/openapi.json").await, StatusCode::OK);
}

#[tokio::test]
async fn power_two_to_the_ten_is_1024() {
    let multiply = spawn_computing_multiply().await;
    let (status, body) = eval(&multiply, 2, 10).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["base"], 2);
    assert_eq!(body["exp"], 10);
    assert_eq!(body["result"], 1024);
}

#[tokio::test]
async fn power_five_to_the_zero_is_one_with_no_calls() {
    // exp == 0 must short-circuit to 1 without ever calling the dependency, so a
    // dead multiply URL must still succeed.
    let (status, body) = eval(DEAD_URL, 5, 0).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], 1);
}

#[tokio::test]
async fn power_three_to_the_three_is_27() {
    let multiply = spawn_computing_multiply().await;
    let (status, body) = eval(&multiply, 3, 3).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], 27);
}

#[tokio::test]
async fn negative_exponent_is_unprocessable() {
    // Rejected before any call, so a dead dependency is irrelevant.
    let (status, body) = eval(DEAD_URL, 2, -1).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"], "negative exponent");
}

#[tokio::test]
async fn degrades_when_multiply_is_unreachable() {
    // exp >= 1 forces a real call; a dead dependency surfaces as 503.
    let (status, body) = eval(DEAD_URL, 2, 3).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["dependency"], "srvcs-multiply");
}
