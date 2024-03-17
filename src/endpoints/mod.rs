//! HTTP endpoints.

use crate::{
    shared::{sql, AppError, AppState, UserInfo, SESSION_USER_INFO_KEY},
    utils::flashed_messages,
};
use anyhow::Result;
use axum::{
    extract::State,
    response::{Html, Redirect},
    routing::{get, post},
    Form, Router,
};
use minijinja::{context, Environment};
use serde::Deserialize;
use std::sync::Arc;
use tower_sessions::Session;

pub mod airspace;
pub mod auth;
pub mod facility;
pub mod homepage;
pub mod user;

/// 404 not found page.
///
/// Redirected to whenever the router cannot find a valid handler for the requested path.
async fn page_404(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Html<String>, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    let template = state.templates.get_template("404")?;
    let rendered = template.render(context! { user_info })?;
    Ok(Html(rendered))
}

/// View the feedback form.
///
/// The template handles requiring the user to be logged in.
async fn page_feedback_form(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Html<String>, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    let flashed_messages = flashed_messages::drain_flashed_messages(session).await?;
    let template = state.templates.get_template("feedback")?;
    let rendered = template.render(context! { user_info, flashed_messages })?;
    Ok(Html(rendered))
}

#[derive(Debug, Deserialize)]
struct FeedbackForm {
    controller: String,
    position: String,
    rating: String,
    comments: String,
}

/// Submit the feedback form.
async fn page_feedback_form_post(
    State(state): State<Arc<AppState>>,
    session: Session,
    Form(feedback): Form<FeedbackForm>,
) -> Result<Redirect, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    if let Some(user_info) = user_info {
        sqlx::query(sql::INSERT_FEEDBACK)
            .bind(feedback.controller)
            .bind(feedback.position)
            .bind(feedback.rating)
            .bind(feedback.comments)
            .bind(sqlx::types::chrono::Utc::now())
            .bind(user_info.cid)
            .execute(&state.db)
            .await?;
        flashed_messages::push_flashed_message(
            session,
            flashed_messages::FlashedMessageLevel::Success,
            "Feedback submitted, thank you!",
        )
        .await?;
    } else {
        flashed_messages::push_flashed_message(
            session,
            flashed_messages::FlashedMessageLevel::Error,
            "You must be logged in to submit feedback.",
        )
        .await?;
    }

    Ok(Redirect::to("/feedback"))
}

/// This file's routes and templates.
pub fn router(templates: &mut Environment) -> Router<Arc<AppState>> {
    templates
        .add_template("404", include_str!("../../templates/404.jinja"))
        .unwrap();
    templates
        .add_template("feedback", include_str!("../../templates/feedback.jinja"))
        .unwrap();

    Router::new()
        .route("/404", get(page_404))
        .route("/feedback", get(page_feedback_form))
        .route("/feedback", post(page_feedback_form_post))
}
