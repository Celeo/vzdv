use crate::{
    shared::{AppError, AppState, UserInfo, SESSION_USER_INFO_KEY},
    utils::vatusa::get_training_notes,
};
use axum::{
    extract::State,
    response::{Html, IntoResponse, Redirect, Response},
    routing::get,
    Router,
};
use minijinja::{context, Environment};
use std::sync::Arc;
use tower_sessions::Session;

async fn page_training_notes(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Response, AppError> {
    let user_info: UserInfo = match session.get(SESSION_USER_INFO_KEY).await? {
        Some(user_info) => user_info,
        None => {
            return Ok(Redirect::to("/").into_response());
        }
    };
    // FIXME restore
    // let records = get_training_notes(&state.config, user_info.cid).await?;
    let training_data = get_training_notes(&state.config, 1640903).await?;
    let records: Vec<_> = training_data.data.iter().rev().collect();
    let template = state.templates.get_template("training_notes")?;
    let rendered = template.render(context! { user_info, records })?;
    Ok(Html(rendered).into_response())
}

pub fn router(templates: &mut Environment) -> Router<Arc<AppState>> {
    templates
        .add_template(
            "training_notes",
            include_str!("../../templates/training_notes.jinja"),
        )
        .unwrap();

    Router::new().route("/user/training_notes", get(page_training_notes))
}
