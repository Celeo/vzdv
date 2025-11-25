//! HTTP endpoints for user-specific pages.

use crate::{
    discord, flashed_messages,
    shared::{AppError, AppState, SESSION_USER_INFO_KEY, UserInfo, record_log, strip_some_tags},
    vatusa::{self, TrainingDataType, TrainingRecord},
};
use axum::{
    Router,
    extract::{Query, State},
    response::{Html, IntoResponse, Redirect, Response},
    routing::get,
};
use itertools::Itertools;
use log::{debug, warn};
use minijinja::context;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use tower_sessions::Session;
use vzdv::{
    sql::{self, AuxiliaryTrainingData, Controller},
    vatusa::get_multiple_controller_names,
};

/// Retrieve and show the user their training records from VATUSA.
async fn page_training_notes(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Response, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    let user_info = match user_info {
        Some(info) => info,
        None => return Ok(Redirect::to("/").into_response()),
    };

    // get the records from VATUSA
    let all_training_records =
        vatusa::get_training_records(user_info.cid, &state.config.vatsim.vatusa_api_key).await?;
    let training_records: Vec<_> = all_training_records
        .iter()
        .filter(|record| record.facility_id == "ZDV")
        .map(|record| {
            let record = record.clone();
            TrainingRecord {
                notes: strip_some_tags(&record.notes).replace("\n", "<br>"),
                ..record
            }
        })
        .collect();

    // include aux data from DB
    let aux_training_data: Vec<AuxiliaryTrainingData> =
        sqlx::query_as(sql::GET_AUX_TRAINING_DATA_FOR)
            .bind(user_info.cid)
            .fetch_all(&state.db)
            .await?;

    // combine both Vecs by enum and sort
    let all_training_data: Vec<TrainingDataType> = training_records
        .into_iter()
        .map(TrainingDataType::VatusaRecord)
        .chain(aux_training_data.into_iter().map(TrainingDataType::AuxData))
        .sorted_by(|record_a, record_b| record_b.get_date().cmp(&record_a.get_date()))
        .collect();

    // get trainer cids for name matching
    let trainer_cids: Vec<u32> = all_training_data
        .iter()
        .map(|record| record.trainer())
        .collect::<HashSet<u32>>()
        .iter()
        .copied()
        .collect();
    let trainers = get_multiple_controller_names(&trainer_cids).await;

    let template = state.templates.get_template("user/training_notes.jinja")?;
    let rendered: String = template.render(context! { user_info, all_training_data, trainers })?;
    Ok(Html(rendered).into_response())
}

/// Show the user a link to the Discord server, as well as provide
/// the start of the Discord OAuth flow for account linking.
async fn page_discord(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Response, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    let user_info = match user_info {
        Some(info) => info,
        None => return Ok(Redirect::to("/").into_response()),
    };
    let controller: Controller = sqlx::query_as(sql::GET_CONTROLLER_BY_CID)
        .bind(user_info.cid)
        .fetch_one(&state.db)
        .await?;
    let template = state.templates.get_template("user/discord.jinja")?;
    let flashed_messages = flashed_messages::drain_flashed_messages(session).await?;
    let rendered: String = template.render(context! {
        user_info,
        oauth_link => discord::get_oauth_link(&state.config),
        join_link => &state.config.discord.join_link,
        discord_id => controller.discord_id,
        flashed_messages
    })?;
    Ok(Html(rendered).into_response())
}

/// Navigation from the Discord OAuth flow.
async fn page_discord_callback(
    State(state): State<Arc<AppState>>,
    session: Session,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Redirect, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    let user_info = match user_info {
        Some(info) => info,
        None => {
            warn!("Unknown user hit Discord link callback page");
            flashed_messages::push_flashed_message(
                session,
                flashed_messages::MessageLevel::Error,
                "Not logged in",
            )
            .await?;
            return Ok(Redirect::to("/"));
        }
    };
    if let Some(code) = params.get("code") {
        debug!("Getting Discord info in callback");
        let access_token = discord::code_to_token(code, &state.config).await?;
        let discord_user_id = discord::get_token_user_id(&access_token).await?;
        sqlx::query(sql::SET_CONTROLLER_DISCORD_ID)
            .bind(user_info.cid)
            .bind(&discord_user_id)
            .execute(&state.db)
            .await?;
        flashed_messages::push_flashed_message(
            session,
            flashed_messages::MessageLevel::Info,
            "Discord account linked",
        )
        .await?;
        record_log(
            format!(
                "Set Discord ID for controller {} to {}",
                user_info.cid, discord_user_id,
            ),
            &state.db,
            true,
        )
        .await?;
    } else {
        warn!(
            "Discord callback page hit by {} without code param",
            user_info.cid
        );
        flashed_messages::push_flashed_message(
            session,
            flashed_messages::MessageLevel::Error,
            "Could not link your Discord account - not enough info provided",
        )
        .await?;
    }
    Ok(Redirect::to("/user/discord"))
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/user/training_notes", get(page_training_notes))
        .route("/user/discord", get(page_discord))
        .route("/user/discord/callback", get(page_discord_callback))
}
