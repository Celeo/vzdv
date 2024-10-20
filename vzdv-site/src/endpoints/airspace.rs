//! Endpoints for getting information on the airspace.

use crate::{
    flashed_messages,
    flights::get_relevant_flights,
    shared::{AppError, AppState, CacheEntry, UserInfo, SESSION_USER_INFO_KEY},
};
use axum::{
    extract::State,
    response::{Html, Redirect},
    routing::{get, post},
    Form, Router,
};
use itertools::Itertools;
use log::{info, warn};
use minijinja::{context, Environment};
use serde::Deserialize;
use serde_json::json;
use std::{collections::HashSet, sync::Arc, time::Instant};
use thousands::Separable;
use tower_sessions::Session;
use vatsim_utils::live_api::Vatsim;
use vzdv::{aviation::parse_metar, GENERAL_HTTP_CLIENT};

/// Table of all the airspace's airports.
async fn page_airports(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Html<String>, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    let template = state.templates.get_template("airspace/airports")?;
    let airports = &state.config.airports.all;
    let rendered = template.render(context! { user_info, airports })?;
    Ok(Html(rendered))
}

/// Table of all airspace-relevant flights.
async fn page_flights(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Html<String>, AppError> {
    // cache this endpoint's returned data for 15 seconds
    let cache_key = "ONLINE_FLIGHTS_FULL";
    if let Some(cached) = state.cache.get(&cache_key) {
        let elapsed = Instant::now() - cached.inserted;
        if elapsed.as_secs() < 15 {
            return Ok(Html(cached.data));
        }
        state.cache.invalidate(&cache_key);
    }

    let vatsim_data = Vatsim::new().await?.get_v3_data().await?;
    let flights = {
        let all = get_relevant_flights(&state.config, &vatsim_data.pilots);
        let mut flights = HashSet::with_capacity(all.plan_within.len()); // won't be all of them, but might save one allocation
        flights.extend(all.actually_within);
        flights.extend(all.plan_from);
        flights.extend(all.plan_to);
        flights.extend(all.plan_within);
        let flights: Vec<_> = flights
            .iter()
            .cloned()
            .sorted_by(|a, b| a.callsign.cmp(&b.callsign))
            .collect();
        flights
    };

    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    let template = state.templates.get_template("airspace/flights")?;
    let rendered = template.render(context! { user_info, flights })?;
    state
        .cache
        .insert(cache_key, CacheEntry::new(rendered.clone()));
    Ok(Html(rendered))
}

/// Larger view of the weather.
async fn page_weather(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Html<String>, AppError> {
    // cache this endpoint's returned data for 5 minutes
    let cache_key = "WEATHER_FULL";
    if let Some(cached) = state.cache.get(&cache_key) {
        let elapsed = Instant::now() - cached.inserted;
        if elapsed.as_secs() < 300 {
            return Ok(Html(cached.data));
        }
        state.cache.invalidate(&cache_key);
    }

    let resp = GENERAL_HTTP_CLIENT
        .get(format!(
            "https://metar.vatsim.net/{}",
            state
                .config
                .airports
                .all
                .iter()
                .map(|airport| &airport.code)
                .join(",")
        ))
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(AppError::HttpResponse("METAR API", resp.status().as_u16()));
    }
    let text = resp.text().await?;
    let weather: Vec<_> = text
        .split_terminator('\n')
        .flat_map(|line| {
            parse_metar(line).map_err(|e| {
                let airport = line.split(' ').next().unwrap_or("Unknown");
                warn!("Metar parsing failure for {airport}: {e}");
                e
            })
        })
        .collect();

    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    let template = state.templates.get_template("airspace/weather")?;
    let rendered = template.render(context! { user_info, weather })?;
    state
        .cache
        .insert(cache_key, CacheEntry::new(rendered.clone()));
    Ok(Html(rendered))
}

/// Form for groups to submit requests for staff-ups.
async fn page_staffing_request(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Html<String>, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    let flashed_messages = flashed_messages::drain_flashed_messages(session).await?;
    let template = state.templates.get_template("airspace/staffing_request")?;
    let rendered = template.render(context! { user_info, flashed_messages })?;
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

/// Submit the staffing request form.
async fn page_staffing_request_post(
    State(state): State<Arc<AppState>>,
    session: Session,
    Form(staffing_request): Form<StaffingRequestForm>,
) -> Result<Redirect, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await.unwrap();
    if let Some(user_info) = user_info {
        let resp = GENERAL_HTTP_CLIENT
            .post(&state.config.discord.webhooks.staffing_request)
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
        info!("{} submitted a staffing request", user_info.cid);
        if resp.status().is_success() {
            flashed_messages::push_flashed_message(
                session,
                flashed_messages::MessageLevel::Success,
                "Request submitted",
            )
            .await?;
        } else {
            flashed_messages::push_flashed_message(
                session,
                flashed_messages::MessageLevel::Error,
                "The message could not be processed. You may want to contact the EC (or WM).",
            )
            .await?;
        }
    } else {
        flashed_messages::push_flashed_message(
            session,
            flashed_messages::MessageLevel::Error,
            "You must be logged in to submit a request",
        )
        .await?;
    }
    Ok(Redirect::to("/airspace/staffing_request"))
}

/// This file's routes and templates.
pub fn router(templates: &mut Environment) -> Router<Arc<AppState>> {
    templates
        .add_template(
            "airspace/airports",
            include_str!("../../templates/airspace/airports.jinja"),
        )
        .unwrap();
    templates
        .add_template(
            "airspace/flights",
            include_str!("../../templates/airspace/flights.jinja"),
        )
        .unwrap();
    templates
        .add_template(
            "airspace/staffing_request",
            include_str!("../../templates/airspace/staffing_request.jinja"),
        )
        .unwrap();
    templates
        .add_template(
            "airspace/weather",
            include_str!("../../templates/airspace/weather.jinja"),
        )
        .unwrap();
    templates.add_filter("format_number", |value: u16| value.separate_with_commas());

    Router::new()
        .route("/airspace/airports", get(page_airports))
        .route("/airspace/flights", get(page_flights))
        .route("/airspace/weather", get(page_weather))
        .route("/airspace/staffing_request", get(page_staffing_request))
        .route(
            "/airspace/staffing_request",
            post(page_staffing_request_post),
        )
}
