//! App middleware functions.

use axum::{extract::Request, middleware::Next, response::Response};
use log::{debug, warn};
use std::{collections::HashSet, sync::LazyLock};
use tower_sessions::{Expiry, Session};

static IGNORE_PATHS: LazyLock<HashSet<&str>> = LazyLock::new(|| HashSet::from(["/favicon.ico"]));

/// Cookie expiration duration (hours).
pub const SESSION_INACTIVITY_WINDOW: i64 = 24;

/// Simple logging middleware.
///
/// Logs the method, path, and response code to debug
/// if processing returned a successful code, and to
/// warn otherwise.
pub async fn logging(request: Request, next: Next) -> Response {
    let uri = request.uri().clone();
    let path = uri.path();
    if !IGNORE_PATHS.contains(path) {
        let method = request.method().clone();
        let response = next.run(request).await;
        let s = format!("{} {} {}", method, path, response.status().as_u16());
        if response.status().is_success() || response.status().is_redirection() {
            debug!("{s}");
        } else {
            warn!("{s}");
        }
        response
    } else {
        next.run(request).await
    }
}

/// Middleware to extend the `tower_sessions` session cookie on the user's browser.
///
/// The cookie's initial duration covers how long the cookie will last between
/// site visits, as this middleware extends the duration of the cookie by the
/// same amount each time the user visits any page on the site.
///
/// This does touch the DB, which I don't love.
pub async fn extend_session(session: Session, request: Request, next: Next) -> Response {
    session.set_expiry(Some(Expiry::OnInactivity(time::Duration::hours(
        SESSION_INACTIVITY_WINDOW,
    ))));
    next.run(request).await
}
