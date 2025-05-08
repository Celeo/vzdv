//! Endpoints for the integrated IDS.

use crate::shared::{AppError, AppState};
use axum::{Router, extract::Json, routing::post};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Data incoming from vATIS.
#[derive(Debug, Deserialize, Serialize)]
pub struct VatisData {
    pub facility: String,
    pub preset: String,
    #[serde(rename = "atisLetter")]
    pub atis_letter: String,
    #[serde(rename = "atisType")]
    pub atis_type: String,
    #[serde(rename = "airportConditions")]
    pub airport_conditions: String,
    pub notams: String,
    pub timestamp: String,
    pub version: String,
}

/// Receive HTTP POST events from vATIS being ran by facility controllers.
async fn receive_vatis_post(Json(_payload): Json<VatisData>) -> Result<StatusCode, AppError> {
    Ok(StatusCode::OK)
}

/// This file's routes and templates.
pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/ids/vatis/submit", post(receive_vatis_post))
}
