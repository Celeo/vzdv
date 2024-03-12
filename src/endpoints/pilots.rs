use crate::{
    shared::{
        sql::INSERT_FEEDBACK, AppError, AppState, CacheEntry, UserInfo, SESSION_USER_INFO_KEY,
    },
    utils::{flashed_messages, simaware_data, GENERAL_HTTP_CLIENT},
};
use axum::{
    extract::State,
    response::{Html, Redirect},
    routing::{get, post},
    Form, Router,
};
use minijinja::{context, Environment};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{sync::Arc, time::Instant};
use thousands::Separable;
use tower_sessions::Session;
use vatsim_utils::live_api::Vatsim;

/// View the feedback form.
///
/// The template handles requiring the user to be logged in.
async fn page_feedback_form(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Html<String>, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await.unwrap();
    let flashed_messages = flashed_messages::drain_flashed_messages(session).await?;
    let template = state.templates.get_template("feedback").unwrap();
    let rendered = template
        .render(context! { user_info, flashed_messages })
        .unwrap();
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
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await.unwrap();
    if let Some(user_info) = user_info {
        sqlx::query(INSERT_FEEDBACK)
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

    Ok(Redirect::to("/pilots/feedback"))
}

/// Table of all the airspace's airports.
async fn page_airports(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Html<String>, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await.unwrap();
    let template = state.templates.get_template("airports").unwrap();
    let airports = &state.config.airports.all;
    let rendered = template.render(context! { user_info, airports }).unwrap();
    Ok(Html(rendered))
}

/// Table of all airspace-relevant flights.
async fn page_flights(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Html<String>, AppError> {
    #[derive(Serialize, Default)]
    struct OnlineFlight<'a> {
        pilot_name: &'a str,
        pilot_cid: u64,
        callsign: &'a str,
        departure: &'a str,
        arrival: &'a str,
        altitude: String,
        speed: String,
        simaware_id: &'a str,
    }

    // cache this endpoint's returned data for 60 seconds
    let cache_key = "ONLINE_FLIGHTS_FULL";
    if let Some(cached) = state.cache.get(&cache_key) {
        let elapsed = Instant::now() - cached.inserted;
        if elapsed.as_secs() < 60 {
            return Ok(Html(cached.data));
        }
        state.cache.invalidate(&cache_key);
    }

    let artcc_fields: Vec<_> = state
        .config
        .airports
        .all
        .iter()
        .map(|airport| &airport.code)
        .collect();
    let vatsim_data = Vatsim::new().await?.get_v3_data().await?;
    let simaware_data = simaware_data().await?;
    let flights: Vec<OnlineFlight> = vatsim_data
        .pilots
        .iter()
        .flat_map(|flight| {
            if let Some(plan) = &flight.flight_plan {
                let from = artcc_fields.contains(&&plan.departure);
                let to = artcc_fields.contains(&&plan.arrival);
                if from || to {
                    Some(OnlineFlight {
                        pilot_name: &flight.name,
                        pilot_cid: flight.cid,
                        callsign: &flight.callsign,
                        departure: &plan.departure,
                        arrival: &plan.arrival,
                        altitude: flight.altitude.separate_with_commas(),
                        speed: flight.groundspeed.separate_with_commas(),
                        simaware_id: match simaware_data.get(&flight.cid) {
                            Some(id) => id,
                            None => "",
                        },
                    })
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await.unwrap();
    let template = state.templates.get_template("flights")?;
    let rendered = template.render(context! { user_info, flights })?;
    state
        .cache
        .insert(cache_key, CacheEntry::new(rendered.clone()));
    Ok(Html(rendered))
}

async fn page_staffing_request(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Html<String>, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await.unwrap();
    let flashed_messages = flashed_messages::drain_flashed_messages(session).await?;
    let template = state.templates.get_template("staffing_request").unwrap();
    let rendered = template
        .render(context! { user_info, flashed_messages })
        .unwrap();
    Ok(Html(rendered))
}

#[derive(Debug, Deserialize)]
struct StaffingRequestForm {
    departure: String,
    dt_start: String,
    pilot_count: i16,
    contact: String,
    arrival: String,
    dt_end: String,
    banner: String,
    organization: String,
    comments: String,
}

async fn page_staffing_request_post(
    State(state): State<Arc<AppState>>,
    session: Session,
    Form(staffing_request): Form<StaffingRequestForm>,
) -> Result<Redirect, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await.unwrap();
    if let Some(user_info) = user_info {
        let resp = GENERAL_HTTP_CLIENT
            .post(&state.config.webhooks.staffing_request)
            .json(&json!({
                "content": "",
                "embeds": [{
                    "title": "New staffing request",
                    "fields": [
                        {
                            "name": "From",
                            "value": format!("{} {} ({})", user_info.first_name, user_info.last_name, user_info.cid)
                        },
                        {
                            "name": "departure",
                            "value": staffing_request.departure
                        },
                        {
                            "name": "arrival",
                            "value": staffing_request.arrival
                        },
                        {
                            "name": "dt_start",
                            "value": staffing_request.dt_start
                        },
                        {
                            "name": "dt_end",
                            "value": staffing_request.dt_end
                        },
                        {
                            "name": "pilot_count",
                            "value": staffing_request.pilot_count
                        },
                        {
                            "name": "contact",
                            "value": staffing_request.contact
                        },
                        {
                            "name": "banner",
                            "value": staffing_request.banner
                        },
                        {
                            "name": "organization",
                            "value": staffing_request.organization
                        },
                        {
                            "name": "comments",
                            "value": staffing_request.comments
                        }
                    ]
                }]
            }))
            .send()
            .await?;
        if resp.status().is_success() {
            flashed_messages::push_flashed_message(
                session,
                flashed_messages::FlashedMessageLevel::Success,
                "Request submitted",
            )
            .await?;
        } else {
            flashed_messages::push_flashed_message(
                session,
                flashed_messages::FlashedMessageLevel::Error,
                "The message could not be processed. You may want to contact the EC (or WM).",
            )
            .await?;
        }
    } else {
        flashed_messages::push_flashed_message(
            session,
            flashed_messages::FlashedMessageLevel::Error,
            "You must be logged in to submit a request",
        )
        .await?;
    }
    Ok(Redirect::to("/pilots/staffing_request"))
}

/// This file's routes and templates.
pub fn router(templates: &mut Environment) -> Router<Arc<AppState>> {
    templates
        .add_template("feedback", include_str!("../../templates/feedback.jinja"))
        .unwrap();
    templates
        .add_template("airports", include_str!("../../templates/airports.jinja"))
        .unwrap();
    templates
        .add_template("flights", include_str!("../../templates/flights.jinja"))
        .unwrap();
    templates
        .add_template(
            "staffing_request",
            include_str!("../../templates/staffing_request.jinja"),
        )
        .unwrap();

    Router::new()
        .route("/pilots/feedback", get(page_feedback_form))
        .route("/pilots/feedback", post(page_feedback_form_post))
        .route("/pilots/airports", get(page_airports))
        .route("/pilots/flights", get(page_flights))
        .route("/pilots/staffing_request", get(page_staffing_request))
        .route("/pilots/staffing_request", post(page_staffing_request_post))
}