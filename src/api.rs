use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use utoipa::{OpenApi, ToSchema};

use crate::client::{self, DepError};

pub const SERVICE: &str = "srvcs-power";
pub const CONCERN: &str = "arithmetic: base raised to exp";
pub const DEPENDS_ON: &[&str] = &["srvcs-multiply"];

/// Dependency endpoints, injected as router state so tests can point them at
/// mock services.
#[derive(Clone)]
pub struct Deps {
    pub multiply_url: String,
}

#[derive(Serialize, ToSchema)]
pub struct Info {
    pub service: &'static str,
    pub concern: &'static str,
    pub depends_on: Vec<&'static str>,
}

/// `GET /` — service identity (srvcs service standard).
#[utoipa::path(get, path = "/", responses((status = 200, body = Info)))]
pub async fn index() -> Json<Info> {
    Json(Info {
        service: SERVICE,
        concern: CONCERN,
        depends_on: DEPENDS_ON.to_vec(),
    })
}

#[derive(Deserialize, ToSchema)]
pub struct EvalRequest {
    pub base: i64,
    pub exp: i64,
}

#[derive(Serialize, ToSchema)]
pub struct PowerResponse {
    pub base: i64,
    pub exp: i64,
    pub result: i64,
}

fn ok(base: i64, exp: i64, result: i64) -> Response {
    (
        StatusCode::OK,
        Json(json!({ "base": base, "exp": exp, "result": result })),
    )
        .into_response()
}

fn invalid(reason: &str) -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(json!({ "error": reason })),
    )
        .into_response()
}

fn degraded(dependency: &str) -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({ "error": "dependency unavailable", "dependency": dependency })),
    )
        .into_response()
}

/// Forward a dependency's response verbatim (e.g. to propagate a `422` raised by
/// the leaf dependency for inputs it considers unprocessable).
fn forward(status: u16, body: Value) -> Response {
    let code = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY);
    (code, Json(body)).into_response()
}

/// Ask `srvcs-multiply` for one product, mapping its failures to the response
/// this service should return.
async fn ask(url: &str, payload: &Value, dependency: &str) -> Result<i64, Response> {
    match client::call(url, payload).await {
        Err(DepError::Unreachable) => Err(degraded(dependency)),
        Ok((200, body)) => Ok(body.get("result").and_then(Value::as_i64).unwrap_or(0)),
        // Invalid input propagates from the leaf dependency; forward it.
        Ok((422, body)) => Err(forward(422, body)),
        Ok(_) => Err(degraded(dependency)),
    }
}

/// `POST /` — compute `base` raised to `exp`.
///
/// This service does no arithmetic of its own. It folds `exp` repeated calls to
/// `srvcs-multiply` over an accumulator seeded at `1`:
/// `acc = 1; for _ in 0..exp { acc = multiply(acc, base) }`. As a result
/// `power(base, 0) == 1` makes no dependency calls at all. A negative exponent
/// is undefined over the integers and is rejected with `422` before any call.
#[utoipa::path(
    post,
    path = "/",
    request_body = EvalRequest,
    responses(
        (status = 200, body = PowerResponse),
        (status = 422, description = "exp is negative, or a dependency rejected an operand (forwarded)"),
        (status = 500, description = "a dependency returned a malformed response"),
        (status = 503, description = "a dependency is unavailable")
    )
)]
pub async fn evaluate(State(deps): State<Deps>, Json(req): Json<EvalRequest>) -> Response {
    if req.exp < 0 {
        return invalid("negative exponent");
    }

    let mut acc: i64 = 1;
    for _ in 0..req.exp {
        acc = match ask(
            &deps.multiply_url,
            &json!({ "a": acc, "b": req.base }),
            "srvcs-multiply",
        )
        .await
        {
            Ok(v) => v,
            Err(resp) => return resp,
        };
    }

    ok(req.base, req.exp, acc)
}

#[derive(OpenApi)]
#[openapi(
    paths(index, evaluate),
    components(schemas(Info, EvalRequest, PowerResponse))
)]
pub struct ApiDoc;

/// Serve OpenAPI document
pub async fn openapi_json() -> Json<utoipa::openapi::OpenApi> {
    Json(ApiDoc::openapi())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openapi_documents_routes() {
        let doc = ApiDoc::openapi();
        let root = doc.paths.paths.get("/").expect("path / present");
        assert!(root.get.is_some());
        assert!(root.post.is_some());
    }

    #[tokio::test]
    async fn index_reports_dependency() {
        let Json(info) = index().await;
        assert_eq!(info.service, "srvcs-power");
        assert_eq!(info.depends_on, vec!["srvcs-multiply"]);
    }
}
