//! HTTP endpoints for the homepage.

use crate::{
    flashed_messages,
    flights::get_relevant_flights,
    shared::{AppError, AppState, CacheEntry, UserInfo, SESSION_USER_INFO_KEY},
};
use axum::{extract::State, response::Html, routing::get, Router};
use chrono::Utc;
use log::warn;
use minijinja::{context, Environment};
use serde::Serialize;
use std::{sync::Arc, time::Instant};
use tower_sessions::Session;
use vatsim_utils::live_api::Vatsim;
use vzdv::{
    aviation::parse_metar,
    sql::{self, Activity},
    vatsim::get_online_facility_controllers,
    GENERAL_HTTP_CLIENT,
};

/// Homepage.
async fn page_home(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Html<String>, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    let template = state.templates.get_template("homepage/home")?;
    let flashed_messages = flashed_messages::drain_flashed_messages(session).await?;
    let rendered = template.render(context! { user_info, flashed_messages })?;
    Ok(Html(rendered))
}

/// Render a list of online controllers.
async fn snippet_online_controllers(
    State(state): State<Arc<AppState>>,
) -> Result<Html<String>, AppError> {
    // cache this endpoint's returned data for 30 seconds
    let cache_key = "ONLINE_CONTROLLERS";
    if let Some(cached) = state.cache.get(&cache_key) {
        let elapsed = Instant::now() - cached.inserted;
        if elapsed.as_secs() < 30 {
            return Ok(Html(cached.data));
        }
        state.cache.invalidate(&cache_key);
    }

    let online = get_online_facility_controllers(&state.db, &state.config)
        .await
        .map_err(|error| AppError::GenericFallback("getting online controllers", error))?;
    let template = state
        .templates
        .get_template("homepage/online_controllers")?;
    let rendered = template.render(context! { online })?;
    state
        .cache
        .insert(cache_key, CacheEntry::new(rendered.clone()));
    Ok(Html(rendered))
}

async fn snippet_weather(State(state): State<Arc<AppState>>) -> Result<Html<String>, AppError> {
    // cache this endpoint's returned data for 5 minutes
    let cache_key = "WEATHER_BRIEF";
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
            state.config.airports.weather_for.join(",")
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
                warn!("METAR parsing failure for {airport}: {e}");
                e
            })
        })
        .collect();

    let template = state.templates.get_template("homepage/weather")?;
    let rendered = template.render(context! { weather })?;
    state
        .cache
        .insert(cache_key, CacheEntry::new(rendered.clone()));
    Ok(Html(rendered))
}

async fn snippet_flights(State(state): State<Arc<AppState>>) -> Result<Html<String>, AppError> {
    #[derive(Serialize, Default)]
    struct OnlineFlights {
        plan_within: usize,
        plan_from: usize,
        plan_to: usize,
        actually_within: usize,
    }

    // cache this endpoint's returned data for 15 seconds
    let cache_key = "ONLINE_FLIGHTS_HOMEPAGE";
    if let Some(cached) = state.cache.get(&cache_key) {
        let elapsed = Instant::now() - cached.inserted;
        if elapsed.as_secs() < 15 {
            return Ok(Html(cached.data));
        }
        state.cache.invalidate(&cache_key);
    }

    let data = Vatsim::new().await?.get_v3_data().await?;
    let flights = get_relevant_flights(&state.config, &data.pilots);
    let flights = OnlineFlights {
        plan_within: flights.plan_within.len(),
        plan_from: flights.plan_from.len(),
        plan_to: flights.plan_to.len(),
        actually_within: flights.actually_within.len(),
    };

    let template = state.templates.get_template("homepage/flights")?;
    let rendered = template.render(context! { flights })?;
    state
        .cache
        .insert(cache_key, CacheEntry::new(rendered.clone()));
    Ok(Html(rendered))
}

async fn snippet_cotm(State(state): State<Arc<AppState>>) -> Result<Html<String>, AppError> {
    // cache this endpoint's returned data for 1 minute
    let cache_key = "COTM";
    if let Some(cached) = state.cache.get(&cache_key) {
        let elapsed = Instant::now() - cached.inserted;
        if elapsed.as_secs() < 60 {
            return Ok(Html(cached.data));
        }
        state.cache.invalidate(&cache_key);
    }

    #[derive(Serialize)]
    struct CotmEntry {
        name: String,
        hours: u32,
        minutes: u32,
    }

    let this_month = Utc::now().format("%Y-%m").to_string();
    let activity: Vec<Activity> = sqlx::query_as(sql::GET_ACTIVITY_IN_MONTH)
        .bind(this_month)
        .fetch_all(&state.db)
        .await?;
    let cotm: Vec<_> = activity
        .iter()
        .take(3)
        .map(|activity| CotmEntry {
            name: format!("{} {}", activity.first_name, activity.last_name),
            hours: activity.minutes / 60,
            minutes: activity.minutes % 60,
        })
        .collect();

    let template = state.templates.get_template("homepage/cotm")?;
    let rendered = template.render(context! { cotm })?;
    state
        .cache
        .insert(cache_key, CacheEntry::new(rendered.clone()));
    Ok(Html(rendered))
}

/// This file's routes and templates.
pub fn router(templates: &mut Environment) -> Router<Arc<AppState>> {
    templates
        .add_template(
            "homepage/home",
            include_str!("../../templates/homepage/home.jinja"),
        )
        .unwrap();
    templates
        .add_template(
            "homepage/online_controllers",
            include_str!("../../templates/homepage/online_controllers.jinja"),
        )
        .unwrap();
    templates
        .add_template(
            "homepage/weather",
            include_str!("../../templates/homepage/weather.jinja"),
        )
        .unwrap();
    templates
        .add_template(
            "homepage/flights",
            include_str!("../../templates/homepage/flights.jinja"),
        )
        .unwrap();
    templates
        .add_template(
            "homepage/cotm",
            include_str!("../../templates/homepage/cotm.jinja"),
        )
        .unwrap();

    Router::new()
        .route("/", get(page_home))
        .route("/home/online/controllers", get(snippet_online_controllers))
        .route("/home/online/flights", get(snippet_flights))
        .route("/home/weather", get(snippet_weather))
        .route("/home/cotm", get(snippet_cotm))
}
