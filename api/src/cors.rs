//! Opt-in CORS configuration.
//!
//! This API has no authentication. With no CORS headers, a browser's
//! same-origin policy stops a page on another origin from reading a response
//! body from this API. `Access-Control-Allow-Origin: *` lifts that restriction
//! for every origin.
//!
//! The default is no CORS layer, which suits development: the frontend calls
//! `/api` on its own origin and Vite proxies it to this API.
//!
//! A deployment serving the frontend and the API as separate services needs
//! this set. The frontend reads the API's address from `/config.js` at runtime,
//! so a different host or port there is a different origin, and every call it
//! makes is cross-origin.
//!
//! `LEDGER_CORS_ALLOWED_ORIGINS` takes a comma-separated list of the origins
//! the frontend is served from:
//!
//! ```text
//! LEDGER_CORS_ALLOWED_ORIGINS=http://10.0.0.5:8091
//! ```
//!
//! An unparseable value fails startup.

use axum::http::{header, HeaderValue, Method};
use std::fmt;
use tower_http::cors::{AllowOrigin, CorsLayer};

pub const ENV_VAR: &str = "LEDGER_CORS_ALLOWED_ORIGINS";

/// A rejected `LEDGER_CORS_ALLOWED_ORIGINS` value. Carries the offending
/// origin.
#[derive(Debug, PartialEq, Eq)]
pub enum CorsConfigError {
    /// The origin is not `scheme://host[:port]`.
    InvalidOrigin(String),
}

impl fmt::Display for CorsConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidOrigin(origin) => write!(
                f,
                "{ENV_VAR} contains an invalid origin: {origin:?}. An origin is a \
                 scheme, host and optional port, with no trailing slash and no \
                 path, like \"https://ledger.example.com\" or \
                 \"http://localhost:5173\". \"*\" allows any origin; this API has \
                 no authentication, so that lets any website read your data."
            ),
        }
    }
}

impl std::error::Error for CorsConfigError {}

/// Reads [`ENV_VAR`] and builds the layer it describes.
///
/// `Ok(None)` means the variable is unset or empty, and no CORS layer is
/// applied.
pub fn layer_from_env() -> Result<Option<CorsLayer>, CorsConfigError> {
    match std::env::var(ENV_VAR) {
        Ok(spec) => layer_from_spec(&spec),
        Err(_) => Ok(None),
    }
}

/// The pure half of [`layer_from_env`], taking the value instead of reading it.
pub fn layer_from_spec(spec: &str) -> Result<Option<CorsLayer>, CorsConfigError> {
    let Some(origins) = parse_origins(spec)? else {
        return Ok(None);
    };

    // The four methods the routes use, and the one header a request body needs.
    // Every body is JSON or multipart.
    Ok(Some(
        CorsLayer::new()
            .allow_origin(origins)
            .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::DELETE])
            .allow_headers([header::CONTENT_TYPE]),
    ))
}

/// Parses the list. An empty or whitespace-only value yields `Ok(None)`, the
/// same as an unset one.
fn parse_origins(spec: &str) -> Result<Option<AllowOrigin>, CorsConfigError> {
    let entries: Vec<&str> = spec
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .collect();

    if entries.is_empty() {
        return Ok(None);
    }

    // "*" allows any origin and must appear alone.
    if entries.contains(&"*") {
        if entries.len() > 1 {
            return Err(CorsConfigError::InvalidOrigin(spec.trim().to_string()));
        }
        return Ok(Some(AllowOrigin::any()));
    }

    let mut origins = Vec::with_capacity(entries.len());
    for entry in entries {
        origins.push(parse_origin(entry)?);
    }
    Ok(Some(AllowOrigin::list(origins)))
}

/// An `Origin` header is `scheme://host[:port]`, with no path and no trailing
/// slash. Anything else cannot match a request.
fn parse_origin(entry: &str) -> Result<HeaderValue, CorsConfigError> {
    let invalid = || CorsConfigError::InvalidOrigin(entry.to_string());

    let authority = entry
        .strip_prefix("http://")
        .or_else(|| entry.strip_prefix("https://"))
        .ok_or_else(invalid)?;

    if authority.is_empty() || authority.contains('/') {
        return Err(invalid());
    }

    HeaderValue::from_str(entry).map_err(|_| invalid())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::routing::get;
    use axum::Router;
    use tower::ServiceExt;

    const ALLOW_ORIGIN: &str = "access-control-allow-origin";

    #[test]
    fn an_unset_variable_means_no_layer() {
        assert!(layer_from_spec("").unwrap().is_none());
    }

    #[test]
    fn a_whitespace_only_value_is_treated_as_unset() {
        assert!(layer_from_spec("   ").unwrap().is_none());
        assert!(layer_from_spec(" , , ").unwrap().is_none());
    }

    #[test]
    fn a_single_origin_is_accepted() {
        assert!(layer_from_spec("https://ledger.example.com")
            .unwrap()
            .is_some());
    }

    #[test]
    fn several_origins_are_accepted_and_surrounding_space_is_ignored() {
        assert!(
            layer_from_spec(" http://localhost:5173 , https://ledger.example.com ")
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn a_lone_wildcard_is_accepted() {
        assert!(layer_from_spec("*").unwrap().is_some());
    }

    #[test]
    fn a_wildcard_mixed_with_named_origins_is_refused() {
        assert_eq!(
            layer_from_spec("*,https://ledger.example.com").unwrap_err(),
            CorsConfigError::InvalidOrigin("*,https://ledger.example.com".to_string())
        );
    }

    #[test]
    fn an_origin_without_a_scheme_is_refused() {
        assert_eq!(
            layer_from_spec("ledger.example.com").unwrap_err(),
            CorsConfigError::InvalidOrigin("ledger.example.com".to_string())
        );
    }

    #[test]
    fn a_non_http_scheme_is_refused() {
        assert_eq!(
            layer_from_spec("ftp://ledger.example.com").unwrap_err(),
            CorsConfigError::InvalidOrigin("ftp://ledger.example.com".to_string())
        );
    }

    /// A browser sends `Origin: https://host` with no trailing slash.
    #[test]
    fn a_trailing_slash_is_refused() {
        assert_eq!(
            layer_from_spec("https://ledger.example.com/").unwrap_err(),
            CorsConfigError::InvalidOrigin("https://ledger.example.com/".to_string())
        );
    }

    #[test]
    fn an_origin_with_a_path_is_refused() {
        assert_eq!(
            layer_from_spec("https://ledger.example.com/api").unwrap_err(),
            CorsConfigError::InvalidOrigin("https://ledger.example.com/api".to_string())
        );
    }

    #[test]
    fn a_scheme_with_no_host_is_refused() {
        assert_eq!(
            layer_from_spec("https://").unwrap_err(),
            CorsConfigError::InvalidOrigin("https://".to_string())
        );
    }

    #[test]
    fn one_bad_origin_refuses_the_whole_list_and_names_itself() {
        assert_eq!(
            layer_from_spec("https://good.example.com,nope").unwrap_err(),
            CorsConfigError::InvalidOrigin("nope".to_string())
        );
    }

    /// The message is the only output before the process exits.
    #[test]
    fn the_error_message_names_the_variable_and_the_bad_value() {
        let message = CorsConfigError::InvalidOrigin("nope".to_string()).to_string();
        assert!(message.contains(ENV_VAR));
        assert!(message.contains("\"nope\""));
    }

    /// Routed through a real request so the assertions are about the response
    /// headers a browser sees, not the shape of the layer.
    async fn allow_origin_header(spec: &str, origin: &str) -> Option<String> {
        let layer = layer_from_spec(spec).unwrap();
        let mut router = Router::new().route("/health", get(|| async { "ok" }));
        if let Some(layer) = layer {
            router = router.layer(layer);
        }

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .header("origin", origin)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        response
            .headers()
            .get(ALLOW_ORIGIN)
            .map(|value| value.to_str().unwrap().to_string())
    }

    #[tokio::test]
    async fn a_configured_origin_is_allowed_to_read_the_response() {
        assert_eq!(
            allow_origin_header("https://ledger.example.com", "https://ledger.example.com").await,
            Some("https://ledger.example.com".to_string())
        );
    }

    #[tokio::test]
    async fn an_origin_not_in_the_list_gets_no_allow_header() {
        assert_eq!(
            allow_origin_header("https://ledger.example.com", "https://evil.example.com").await,
            None
        );
    }

    /// A different port is a different origin.
    #[tokio::test]
    async fn a_matching_host_on_another_port_gets_no_allow_header() {
        assert_eq!(
            allow_origin_header("http://localhost:5173", "http://localhost:5174").await,
            None
        );
    }

    /// A different scheme is a different origin.
    #[tokio::test]
    async fn a_matching_host_on_another_scheme_gets_no_allow_header() {
        assert_eq!(
            allow_origin_header("https://ledger.example.com", "http://ledger.example.com").await,
            None
        );
    }

    #[tokio::test]
    async fn each_origin_in_a_list_is_allowed() {
        let spec = "http://localhost:5173,https://ledger.example.com";
        assert_eq!(
            allow_origin_header(spec, "http://localhost:5173").await,
            Some("http://localhost:5173".to_string())
        );
        assert_eq!(
            allow_origin_header(spec, "https://ledger.example.com").await,
            Some("https://ledger.example.com".to_string())
        );
    }

    #[tokio::test]
    async fn a_wildcard_allows_an_origin_that_was_never_configured() {
        assert_eq!(
            allow_origin_header("*", "https://evil.example.com").await,
            Some("*".to_string())
        );
    }

    #[tokio::test]
    async fn an_unset_variable_sends_no_allow_header() {
        assert_eq!(
            allow_origin_header("", "https://ledger.example.com").await,
            None
        );
    }

    /// A preflight is answered by the layer and never reaches the route.
    #[tokio::test]
    async fn a_preflight_from_a_configured_origin_advertises_the_allowed_methods() {
        let layer = layer_from_spec("https://ledger.example.com")
            .unwrap()
            .unwrap();
        let router = Router::new()
            .route("/health", get(|| async { "ok" }))
            .layer(layer);

        let response = router
            .oneshot(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/health")
                    .header("origin", "https://ledger.example.com")
                    .header("access-control-request-method", "POST")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let headers = response.headers();
        assert_eq!(
            headers.get(ALLOW_ORIGIN).unwrap(),
            "https://ledger.example.com"
        );
        let methods = headers
            .get("access-control-allow-methods")
            .unwrap()
            .to_str()
            .unwrap();
        for method in ["GET", "POST", "PATCH", "DELETE"] {
            assert!(methods.contains(method), "{methods} is missing {method}");
        }
        assert!(!methods.contains("PUT"), "{methods} should not allow PUT");
    }

    #[tokio::test]
    async fn a_preflight_from_an_unknown_origin_gets_no_allow_header() {
        let layer = layer_from_spec("https://ledger.example.com")
            .unwrap()
            .unwrap();
        let router = Router::new()
            .route("/health", get(|| async { "ok" }))
            .layer(layer);

        let response = router
            .oneshot(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/health")
                    .header("origin", "https://evil.example.com")
                    .header("access-control-request-method", "POST")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert!(response.headers().get(ALLOW_ORIGIN).is_none());
    }
}
