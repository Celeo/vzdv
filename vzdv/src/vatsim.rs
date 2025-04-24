use crate::{
    GENERAL_HTTP_CLIENT, config::Config, get_controller_cids_and_names,
    position_in_facility_airspace,
};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use log::error;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::SqlitePool;
use std::collections::HashMap;
use vatsim_utils::live_api::Vatsim;

/// Parse a VATSIM timestamp into a `chrono::DateTime`.
pub fn parse_vatsim_timestamp(stamp: &str) -> Result<DateTime<Utc>> {
    let naive = NaiveDateTime::parse_from_str(stamp, "%Y-%m-%dT%H:%M:%S%.fZ")?;
    let utc = match Utc.from_local_datetime(&naive) {
        chrono::LocalResult::Single(t) => t,
        _ => {
            bail!("Could not parse VATSIM timestamp");
        }
    };
    Ok(utc)
}

#[derive(Debug, Serialize)]
pub struct OnlineController {
    pub cid: u32,
    pub callsign: String,
    pub name: String,
    pub online_for: String,
    pub frequency: String,
}

/// Get facility controllers currently online.
pub async fn get_online_facility_controllers(
    db: &SqlitePool,
    config: &Config,
) -> Result<Vec<OnlineController>> {
    let cid_name_map = match get_controller_cids_and_names(db).await {
        Ok(map) => map,
        Err(e) => {
            error!("Error generating controller CID -> name map: {e}");
            HashMap::new()
        }
    };

    let now = chrono::Utc::now();
    let data = Vatsim::new()
        .await
        .context("determining VATSIM live endpoint")?
        .get_v3_data()
        .await
        .context("getting v3 live data")?;
    let online: Vec<_> = data
        .controllers
        .iter()
        .filter(|controller| {
            position_in_facility_airspace(config, &controller.callsign)
                && controller.frequency != "199.998"
        })
        .map(|controller| {
            let logon = parse_vatsim_timestamp(&controller.logon_time)
                .expect("Could not parse VATSIM timestamp");
            let seconds = (now - logon).num_seconds() as u32;
            OnlineController {
                cid: controller.cid as u32,
                callsign: controller.callsign.clone(),
                name: cid_name_map
                    .get(&(controller.cid as u32))
                    .map(|s| format!("{} {}", s.0, s.1))
                    .unwrap_or(String::from("?")),
                online_for: format!("{}h{}m", seconds / 3600, (seconds / 60) % 60),
                frequency: controller.frequency.clone(),
            }
        })
        .collect();

    Ok(online)
}

#[derive(Debug, Deserialize)]
pub struct AuthCallback {
    pub code: String,
}

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub scopes: Vec<String>,
    pub token_type: String,
    pub expires_in: u64,
    pub access_token: String,
    pub refresh_token: String,
}

#[derive(Debug, Deserialize)]
pub struct UserInfoResponse {
    pub data: UserInfoResponseData,
}

#[derive(Debug, Deserialize)]
pub struct UserInfoResponseData {
    pub cid: String,
    pub personal: UserInfoResponseDataPersonal,
    pub vatsim: UserInfoResponseDataVatsim,
}

#[derive(Debug, Deserialize)]
pub struct UserInfoResponseDataPersonal {
    pub name_first: String,
    pub name_last: String,
    pub name_full: String,
    pub email: String,
}

#[derive(Debug, Deserialize)]
pub struct UserInfoResponseDataVatsim {
    pub rating: IdLongShort,
    #[serde(rename(deserialize = "pilotrating"))]
    pub pilot_rating: IdLongShort,
    pub division: IdName,
    pub region: IdName,
    pub subdivision: IdName,
}

#[derive(Debug, Deserialize)]
pub struct IdLongShort {
    pub id: u32,
    pub long: String,
    pub short: String,
}

#[derive(Debug, Deserialize)]
pub struct IdName {
    pub id: Option<String>,
    pub name: Option<String>,
}

/// Build the URL to redirect users to in order to start
/// their VATSIM OAuth login flow.
pub fn oauth_redirect_start(config: &Config) -> String {
    format!(
        "{}oauth/authorize?response_type=code&client_id={}&redirect_uri={}&scope={}",
        config.vatsim.oauth_url_base,
        config.vatsim.oauth_client_id,
        config.vatsim.oauth_client_callback_url,
        "full_name email vatsim_details"
    )
}

/// Exchange the code from VATSIM OAuth for an access token.
pub async fn code_to_tokens(code: &str, config: &Config) -> Result<TokenResponse> {
    let resp = GENERAL_HTTP_CLIENT
        .post(format!("{}oauth/token", config.vatsim.oauth_url_base))
        .json(&json!({
            "grant_type": "authorization_code",
            "client_id": config.vatsim.oauth_client_id,
            "client_secret": config.vatsim.oauth_client_secret,
            "redirect_uri": config.vatsim.oauth_client_callback_url,
            "code": code
        }))
        .send()
        .await?;
    if !resp.status().is_success() {
        bail!(
            "Got status code {} from VATSIM OAuth exchange",
            resp.status().as_u16()
        );
    }
    let data = resp.json().await?;
    Ok(data)
}

/// Using the user's access token, get their VATSIM info.
pub async fn get_user_info(access_token: &str, config: &Config) -> Result<UserInfoResponse> {
    let resp = GENERAL_HTTP_CLIENT
        .get(format!("{}api/user", config.vatsim.oauth_url_base))
        .header("Authorization", &format!("Bearer {access_token}"))
        .send()
        .await?;
    if !resp.status().is_success() {
        bail!(
            "Got status code {} from VATSIM OAuth user info",
            resp.status().as_u16()
        );
    }
    let data = resp.json().await?;
    Ok(data)
}

#[derive(Debug, Deserialize, Serialize)]
pub struct VatsimMemberInfo {
    id: u32,
    rating: i8,
    #[serde(rename = "pilotrating")]
    pilot_rating: i8,
    #[serde(rename = "militaryrating")]
    military_rating: i8,
    susp_date: Option<serde_json::Value>,
    reg_date: String,
    region_id: Option<String>,
    division_id: Option<String>,
    subdivision_id: Option<String>,
    #[serde(rename = "lastratingchange")]
    last_rating_change: String,
}

/// Get basic VATSIM info for a controller.
pub async fn get_member_info(cid: u32) -> Result<VatsimMemberInfo> {
    let resp = GENERAL_HTTP_CLIENT
        .get(format!("https://api.vatsim.net/v2/members/{cid}"))
        .send()
        .await?;
    if !resp.status().is_success() {
        bail!(
            "Got status code {} from VATSIM basic member info",
            resp.status().as_u16()
        );
    }
    let data = resp.json().await?;
    Ok(data)
}
