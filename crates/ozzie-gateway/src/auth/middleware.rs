use std::sync::Arc;

use axum::body::Body;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use super::{extract_bearer, Authenticator};

/// Device ID stored in request extensions after successful auth.
#[derive(Clone, Debug)]
pub struct DeviceId(pub String);

/// Axum middleware that enforces authentication via Bearer token.
///
/// Token is extracted from:
/// 1. `Authorization: Bearer <token>` header
/// 2. `?token=<token>` query parameter (for WebSocket connections)
///
/// If `authenticator` is `None`, all requests pass (insecure mode).
pub async fn auth_middleware(
    authenticator: Option<Arc<dyn Authenticator>>,
    mut req: Request<Body>,
    next: Next,
) -> Response {
    let Some(auth) = authenticator else {
        // Insecure mode: no auth configured, let everything through.
        req.extensions_mut().insert(DeviceId("anonymous".to_string()));
        return next.run(req).await;
    };

    // Try Authorization header first, then query parameter.
    let token = extract_token_from_header(&req).or_else(|| extract_token_from_query(&req));

    let Some(token) = token else {
        return unauthorized_response();
    };

    match auth.authenticate(&token).await {
        Ok(device_id) => {
            req.extensions_mut().insert(DeviceId(device_id));
            next.run(req).await
        }
        Err(_) => unauthorized_response(),
    }
}

/// Extracts bearer token from the Authorization header.
fn extract_token_from_header(req: &Request<Body>) -> Option<String> {
    let header = req.headers().get("authorization")?.to_str().ok()?;
    extract_bearer(header).map(|s| s.to_string())
}

/// Extracts token from query parameter `?token=<value>`.
fn extract_token_from_query(req: &Request<Body>) -> Option<String> {
    let query = req.uri().query()?;
    for pair in query.split('&') {
        if let Some(value) = pair.strip_prefix("token=")
            && !value.is_empty()
        {
            return Some(value.to_string());
        }
    }
    None
}

/// Returns a 401 Unauthorized JSON response.
fn unauthorized_response() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        axum::Json(serde_json::json!({
            "error": "unauthorized",
            "hint": "provide Authorization: Bearer <token> header (token file: $OZZIE_PATH/.token)"
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use axum::middleware;
    use axum::routing::get;
    use axum::Router;
    use crate::auth::{InsecureAuth, LocalAuth};
    use tower::ServiceExt;

    fn make_router(auth: Option<Arc<dyn Authenticator>>) -> Router {
        let auth_clone = auth.clone();
        Router::new()
            .route("/protected", get(|| async { "ok" }))
            .layer(middleware::from_fn(move |req, next| {
                let auth = auth_clone.clone();
                auth_middleware(auth, req, next)
            }))
    }

    #[tokio::test]
    async fn no_auth_passes_all_requests() {
        let app = make_router(None);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn valid_bearer_token() {
        let auth = LocalAuth::new("test_secret_token");
        let app = make_router(Some(Arc::new(auth)));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", "Bearer test_secret_token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn invalid_bearer_token() {
        let auth = LocalAuth::new("test_secret_token");
        let app = make_router(Some(Arc::new(auth)));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", "Bearer wrong_token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn missing_token() {
        let auth = LocalAuth::new("test_secret_token");
        let app = make_router(Some(Arc::new(auth)));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn token_from_query_param() {
        let auth = LocalAuth::new("query_token_123");
        let app = make_router(Some(Arc::new(auth)));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/protected?token=query_token_123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn invalid_query_token() {
        let auth = LocalAuth::new("real_token");
        let app = make_router(Some(Arc::new(auth)));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/protected?token=wrong")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn insecure_auth_passes_any_token() {
        let auth: Arc<dyn Authenticator> = Arc::new(InsecureAuth);
        let app = make_router(Some(auth));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", "Bearer anything")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn bearer_takes_priority_over_query() {
        let auth = LocalAuth::new("header_token");
        let app = make_router(Some(Arc::new(auth)));

        // Header has correct token, query has wrong token
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/protected?token=wrong")
                    .header("Authorization", "Bearer header_token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn unauthorized_response_is_json() {
        let auth = LocalAuth::new("secret");
        let app = make_router(Some(Arc::new(auth)));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "unauthorized");
        assert!(json["hint"].as_str().unwrap().contains("Bearer"));
    }

    #[test]
    fn extract_token_from_header_fn() {
        let req = Request::builder()
            .header("Authorization", "Bearer my_token")
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_token_from_header(&req), Some("my_token".to_string()));
    }

    #[test]
    fn extract_token_from_query_fn() {
        let req = Request::builder()
            .uri("/ws?token=abc123")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            extract_token_from_query(&req),
            Some("abc123".to_string())
        );
    }

    #[test]
    fn extract_token_from_query_empty() {
        let req = Request::builder()
            .uri("/ws?token=")
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_token_from_query(&req), None);
    }

    #[test]
    fn extract_token_from_query_missing() {
        let req = Request::builder()
            .uri("/ws?other=value")
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_token_from_query(&req), None);
    }

    #[test]
    fn extract_token_from_query_multiple_params() {
        let req = Request::builder()
            .uri("/ws?foo=bar&token=secret&baz=qux")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            extract_token_from_query(&req),
            Some("secret".to_string())
        );
    }

    #[test]
    fn device_id_clone() {
        let id = DeviceId("local".to_string());
        let id2 = id.clone();
        assert_eq!(id.0, id2.0);
    }
}
