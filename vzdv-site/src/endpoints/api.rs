use crate::{
    flights::{get_relevant_flights, OnlineFlights},
    shared::{AppError, AppState, CacheEntry},
};
use axum::{extract::State, routing::get, Json, Router};
use std::{sync::Arc, time::Instant};
use vatsim_utils::live_api::Vatsim;

/// API endpoint to get the flights JSON data.
async fn api_page_flights(
    State(state): State<Arc<AppState>>,
) -> Result<Json<OnlineFlights>, AppError> {
    // cache this endpoint's returned data for 15 seconds
    let cache_key = "ONLINE_FLIGHTS";
    if let Some(cached) = state.cache.get(&cache_key) {
        let elapsed = Instant::now() - cached.inserted;
        if elapsed.as_secs() < 15 {
            return Ok(Json(serde_json::from_str(&cached.data)?));
        }
        state.cache.invalidate(&cache_key);
    }

    let data = Vatsim::new().await?.get_v3_data().await?;
    let flights = get_relevant_flights(&state.config, &data.pilots);
    let as_str = serde_json::to_string(&flights)?;
    state.cache.insert(cache_key, CacheEntry::new(as_str));

    Ok(Json(flights))
}

/// This file's routes and templates.
pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/api/flights", get(api_page_flights))
}
