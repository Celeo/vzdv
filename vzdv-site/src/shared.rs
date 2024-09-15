//! Structs and data to be shared across multiple parts of the site.

use axum::extract::rejection::FormRejection;
use axum::{
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
};
use chrono::{NaiveDateTime, TimeZone};
use log::{error, info};
use mini_moka::sync::Cache;
use minijinja::{context, Environment};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::OnceLock;
use std::{sync::Arc, time::Instant};
use tower_sessions_sqlx_store::sqlx::SqlitePool;
use vzdv::GENERAL_HTTP_CLIENT;
use vzdv::{
    config::Config,
    controller_can_see,
    sql::{self, Controller},
    PermissionsGroup,
};

/// Discord webhook for reporting errors.
///
/// Here as a global since the error handling functions don't
/// otherwise have access to the loaded config struct.
pub static ERROR_WEBHOOK: OnceLock<String> = OnceLock::new();

/// Error handling for all possible issues.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error(transparent)]
    Session(#[from] tower_sessions::session::Error),
    #[error(transparent)]
    Templates(#[from] minijinja::Error),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    HttpCall(#[from] reqwest::Error),
    #[error("remote site query of {0} returned status {1}")]
    HttpResponse(&'static str, u16),
    #[error(transparent)]
    VatsimApi(#[from] vatsim_utils::errors::VatsimUtilError),
    #[error(transparent)]
    ChronoParse(#[from] chrono::ParseError),
    #[error(transparent)]
    ChronoTimezone(#[from] chrono_tz::ParseError),
    #[error("other chrono error")]
    ChronoOther(&'static str),
    #[error(transparent)]
    NumberParsing(#[from] std::num::ParseIntError),
    #[error(transparent)]
    FormExtractionRejection(#[from] FormRejection),
    #[error(transparent)]
    EmailError(#[from] lettre::transport::smtp::Error),
    #[error("unknown email template {0}")]
    UnknownEmailTemplate(String),
    #[error("generic error {0}: {1}")]
    GenericFallback(&'static str, anyhow::Error),
}

impl AppError {
    fn friendly_message(&self) -> &'static str {
        match self {
            Self::Session(_) => "Issue accessing session data",
            Self::Templates(_) => "Issue generating page",
            Self::Database(_) => "Issue accessing database",
            Self::HttpCall(_) => "Issue sending HTTP call",
            Self::HttpResponse(_, _) => "Issue processing HTTP response",
            Self::VatsimApi(_) => "Issue accessing VATSIM APIs",
            Self::ChronoParse(_) => "Issue processing time data",
            Self::ChronoTimezone(_) => "Issue processing timezone data",
            Self::ChronoOther(_) => "Issue processing time",
            Self::NumberParsing(_) => "Issue parsing numbers",
            Self::FormExtractionRejection(_) => "Issue getting info from you",
            Self::EmailError(_) => "Issue sending an email",
            Self::UnknownEmailTemplate(_) => "Unknown email template",
            Self::GenericFallback(_, _) => "Unknown error",
        }
    }
}

/// Try to construct the error page.
fn try_build_error_page(error: AppError) -> Result<String, AppError> {
    let mut env = Environment::new();
    env.add_template("_layout", include_str!("../templates/_layout.jinja"))?;
    env.add_template("_error", include_str!("../templates/_error.jinja"))?;
    let template = env.get_template("_error")?;
    let rendered = template.render(context! { error => error.friendly_message() })?;
    Ok(rendered)
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let error_msg = format!("{self}");
        error!("Unhandled error: {error_msg}");
        let status = match &self {
            Self::FormExtractionRejection(e) => match e {
                FormRejection::FailedToDeserializeForm(_)
                | FormRejection::FailedToDeserializeFormBody(_) => StatusCode::BAD_REQUEST,
                FormRejection::InvalidFormContentType(_) => StatusCode::UNSUPPORTED_MEDIA_TYPE,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            },
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };

        // report errors to Discord webhook
        tokio::spawn(async move {
            if let Some(url) = ERROR_WEBHOOK.get() {
                let res = GENERAL_HTTP_CLIENT
                    .post(url)
                    .json(&json!({
                        "content": format!("Error occurred, returning status {status}: {error_msg}")
                    }))
                    .send()
                    .await;
                if let Err(e) = res {
                    error!("Could not send error to Discord webhook: {e}");
                }
            }
        });

        // attempt to construct the error page, falling back to simple plain text if anything failed
        if let Ok(body) = try_build_error_page(self) {
            (status, Html(body)).into_response()
        } else {
            (status, "Something went very wrong").into_response()
        }
    }
}

/// Data wrapper for items in the server-side cache.
#[derive(Clone)]
pub struct CacheEntry {
    pub inserted: Instant,
    pub data: String,
}

impl CacheEntry {
    /// Wrap the data with a timestamp.
    pub fn new(data: String) -> Self {
        Self {
            inserted: Instant::now(),
            data,
        }
    }
}

/// App's state, available in all handlers via an extractor.
pub struct AppState {
    /// App config
    pub config: Config,
    /// Access to the DB
    pub db: SqlitePool,
    /// Loaded templates
    pub templates: Environment<'static>,
    /// Server-side cache for heavier-compute rendered templates
    pub cache: Cache<&'static str, CacheEntry>,
}

/// Key for user info CRUD in session.
pub const SESSION_USER_INFO_KEY: &str = "USER_INFO";
/// Key for flashed messages CRUD in session.
pub const SESSION_FLASHED_MESSAGES_KEY: &str = "FLASHED_MESSAGES";

/// Data stored in the user's session.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UserInfo {
    pub cid: u32,
    pub first_name: String,
    pub last_name: String,

    pub is_some_staff: bool,
    pub is_training_staff: bool,
    pub is_event_staff: bool,
    pub is_admin: bool,
}

/// Returns a response to redirect to the homepage for non-staff users.
///
/// This function checks the database to ensure that the staff member is
/// still actually a staff member at the time of making the request.
///
/// So long as the permissions being checked against aren't `PermissionsGroup::Anon`,
/// it's safe to assume that `user_info` is `Some<UserInfo>`.
pub async fn reject_if_not_in(
    state: &Arc<AppState>,
    user_info: &Option<UserInfo>,
    permissions: PermissionsGroup,
) -> Option<Redirect> {
    if is_user_member_of(state, user_info, permissions).await {
        None
    } else {
        info!(
            "Rejected access for {} to a resource",
            user_info.as_ref().map(|ui| ui.cid).unwrap_or_default()
        );
        Some(Redirect::to("/"))
    }
}

/// Return whether the user is a member of the corresponding staff group.
///
/// This function checks the database to ensure that the staff member is
/// still actually a staff member at the time of making the request.
///
/// So long as the permissions being checked against aren't `PermissionsGroup::Anon`,
/// it's safe to assume that `user_info` is `Some<UserInfo>`.
pub async fn is_user_member_of(
    state: &Arc<AppState>,
    user_info: &Option<UserInfo>,
    permissions: PermissionsGroup,
) -> bool {
    if user_info.is_none() {
        return false;
    }
    let user_info = user_info.as_ref().unwrap();
    let controller: Option<Controller> = match sqlx::query_as(sql::GET_CONTROLLER_BY_CID)
        .bind(user_info.cid)
        .fetch_optional(&state.db)
        .await
    {
        Ok(c) => c,
        Err(e) => {
            error!("Unknown controller with CID {}: {e}", user_info.cid);
            return false;
        }
    };
    controller_can_see(&controller, permissions)
}

/// Convert an HTML `datetime-local` input and JS timezone name to a UTC timestamp.
///
/// Kind of annoying.
pub fn js_timestamp_to_utc(timestamp: &str, timezone: &str) -> Result<NaiveDateTime, AppError> {
    let tz: chrono_tz::Tz = timezone.parse()?;
    let original = NaiveDateTime::parse_from_str(timestamp, "%Y-%m-%dT%H:%M")?;
    let converted = tz
        .from_local_datetime(&original)
        .single()
        .ok_or_else(|| AppError::ChronoOther("Error parsing HTML datetime"))?
        .naive_utc();
    Ok(converted)
}
