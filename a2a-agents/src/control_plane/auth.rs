//! Bearer-token authentication for the control-plane HTTP API.
//!
//! The control plane is a privileged surface: `POST /agents` starts a process or
//! container of the caller's choosing and hands it the allowlisted secrets the
//! deploying process holds. Unauthenticated, that is remote code execution with
//! the operator's credentials, and binding to loopback is a deployment accident
//! away from being the only thing preventing it. So authentication is
//! **required** and disabling it is an explicit, loud choice.
//!
//! Kept in `a2a-agents` rather than reusing `a2a_rs::adapter::auth`: that
//! middleware is written against axum 0.8 while the control plane (like the rest
//! of this crate's HTTP surface) is on 0.7.

use axum::body::Body;
use axum::extract::Request;
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::{Json, middleware};

/// How the control-plane API authenticates callers.
#[derive(Debug, Clone)]
pub enum ControlPlaneAuth {
    /// Require `Authorization: Bearer <token>` on every request.
    BearerToken(String),
    /// Accept every request. Only for a trusted, isolated dev loop — the caller
    /// has to name this variant, so it can never be reached by forgetting to
    /// configure a token.
    Disabled,
}

impl ControlPlaneAuth {
    /// Require this bearer token.
    pub fn bearer(token: impl Into<String>) -> Self {
        Self::BearerToken(token.into())
    }

    /// Wrap `router` in the authentication layer this variant describes.
    pub(crate) fn apply(self, router: axum::Router) -> axum::Router {
        match self {
            Self::Disabled => router,
            Self::BearerToken(token) => router.layer(middleware::from_fn(
                move |req: Request<Body>, next: Next| {
                    let token = token.clone();
                    async move {
                        match bearer_of(&req) {
                            Some(presented) if constant_time_eq(presented, &token) => {
                                next.run(req).await
                            }
                            _ => unauthorized(),
                        }
                    }
                },
            )),
        }
    }
}

/// The token from an `Authorization: Bearer <token>` header, if well-formed.
fn bearer_of(req: &Request<Body>) -> Option<&str> {
    req.headers()
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::trim)
}

/// Compare two secrets without leaking *where* they differ through timing.
///
/// The length comparison is deliberately not constant-time: token length is not
/// the secret, and the alternative (padding) obscures the code for no gain.
fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// A 401 that does not distinguish "no token" from "wrong token".
fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": "missing or invalid bearer token" })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_eq_matches_string_equality() {
        assert!(constant_time_eq("s3cret", "s3cret"));
        assert!(!constant_time_eq("s3cret", "s3crey"));
        assert!(!constant_time_eq("s3cret", "s3cre"));
        assert!(!constant_time_eq("", "x"));
        assert!(constant_time_eq("", ""));
    }

    /// Build a request carrying `header_value` as `Authorization`.
    fn req_with_auth(header_value: &str) -> Request<Body> {
        Request::builder()
            .header(header::AUTHORIZATION, header_value)
            .body(Body::empty())
            .unwrap()
    }

    #[test]
    fn bearer_is_parsed_only_from_a_well_formed_header() {
        assert_eq!(bearer_of(&req_with_auth("Bearer tok")), Some("tok"));
        // Scheme is case-sensitive per the header's own grammar in our usage,
        // and a bare token or another scheme is not a bearer token.
        assert_eq!(bearer_of(&req_with_auth("bearer tok")), None);
        assert_eq!(bearer_of(&req_with_auth("Basic dXNlcjpwdw==")), None);
        assert_eq!(bearer_of(&req_with_auth("tok")), None);
        assert_eq!(
            bearer_of(&Request::builder().body(Body::empty()).unwrap()),
            None
        );
    }
}
