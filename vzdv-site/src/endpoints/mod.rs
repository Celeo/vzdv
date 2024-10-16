//! HTTP endpoints.

use crate::{
    flashed_messages,
    shared::{AppError, AppState, UserInfo, SESSION_USER_INFO_KEY},
};
use axum::{
    extract::State,
    response::{Html, Redirect},
    routing::{get, post},
    Form, Router,
};
use log::info;
use minijinja::{context, Environment};
use serde::Deserialize;
use std::sync::Arc;
use tower_http::services::ServeDir;
use tower_sessions::Session;
use vzdv::sql::{self, Controller};

pub mod admin;
pub mod airspace;
pub mod auth;
pub mod controller;
pub mod events;
pub mod facility;
pub mod homepage;
pub mod user;

/// 404 not found page.
///
/// Redirected to whenever the router cannot find a valid handler for the requested path.
pub async fn page_404(State(state): State<Arc<AppState>>) -> Result<Html<String>, AppError> {
    let template = state.templates.get_template("404")?;
    let rendered = template.render(context! { no_links => true })?;
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
    let mut all_controllers: Vec<Controller> = sqlx::query_as(sql::GET_ALL_CONTROLLERS_ON_ROSTER)
        .fetch_all(&state.db)
        .await?;
    all_controllers.sort_by_key(|c| format!("{} {}", c.first_name, c.last_name));
    let all_controllers: Vec<(u32, String)> = all_controllers
        .iter()
        .map(|controller| {
            (
                controller.cid,
                format!("{} {}", controller.first_name, controller.last_name,),
            )
        })
        .collect();
    let template = state.templates.get_template("feedback")?;
    let rendered = template.render(context! { user_info, flashed_messages, all_controllers })?;
    Ok(Html(rendered))
}

#[derive(Debug, Deserialize)]
struct FeedbackForm {
    controller: u32,
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
            .bind(&feedback.position)
            .bind(&feedback.rating)
            .bind(&feedback.comments)
            .bind(sqlx::types::chrono::Utc::now())
            .bind(user_info.cid)
            .execute(&state.db)
            .await?;
        info!(
            "{} submitted feedback for {}",
            user_info.cid, feedback.controller
        );
        flashed_messages::push_flashed_message(
            session,
            flashed_messages::MessageLevel::Success,
            "Feedback submitted, thank you!",
        )
        .await?;
    } else {
        flashed_messages::push_flashed_message(
            session,
            flashed_messages::MessageLevel::Error,
            "You must be logged in to submit feedback.",
        )
        .await?;
    }
    Ok(Redirect::to("/feedback"))
}

async fn page_changelog(State(state): State<Arc<AppState>>) -> Result<Html<String>, AppError> {
    let template = state.templates.get_template("changelog")?;
    let rendered = template.render(context! { no_links => true })?;
    Ok(Html(rendered))
}

/// This file's routes and templates.
pub fn router(templates: &mut Environment) -> Router<Arc<AppState>> {
    templates
        .add_template("404", include_str!("../../templates/404.jinja"))
        .unwrap();
    templates
        .add_template("feedback", include_str!("../../templates/feedback.jinja"))
        .unwrap();
    templates
        .add_template("changelog", include_str!("../../templates/changelog.jinja"))
        .unwrap();

    Router::new()
        .route("/404", get(page_404))
        .route("/feedback", get(page_feedback_form))
        .route("/feedback", post(page_feedback_form_post))
        .route("/changelog", get(page_changelog))
        .nest_service("/assets", ServeDir::new("assets"))
}
