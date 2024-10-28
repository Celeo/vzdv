use crate::{
    flashed_messages, load_templates,
    shared::{AppError, AppState, UserInfo, SESSION_USER_INFO_KEY},
};
use axum::{
    response::{Html, IntoResponse, Redirect, Response},
    routing::get,
    Router,
};
use minijinja::context;
use std::sync::Arc;
use tower_sessions::Session;

async fn training_home(session: Session) -> Result<Response, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    let user_info = match user_info {
        Some(ui) => ui,
        None => {
            return Ok(Redirect::to("/").into_response());
        }
    };
    let env = load_templates().unwrap();
    let template = env.get_template("training/home.jinja")?;
    let flashed_messages = flashed_messages::drain_flashed_messages(session).await?;
    let rendered = template.render(context! { user_info, flashed_messages })?;
    Ok(Html(rendered).into_response())
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/training", get(training_home))
}
