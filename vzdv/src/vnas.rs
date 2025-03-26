use crate::{GENERAL_HTTP_CLIENT, config::Config, position_in_facility_airspace};
use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const URL: &str = "https://live.env.vnas.vatsim.net/data-feed/controllers.json";

// More specific enum values available at https://github.com/vatsim-vnas/data-feed/tree/master/ControllerFeed

/// Top-level vNAS data.
#[derive(Debug, Deserialize, Serialize)]
pub struct VNasData {
    #[serde(rename = "updatedAt")]
    updated_at: String,
    controllers: Vec<VNasController>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct VNasController {
    #[serde(rename = "artccId")]
    artcc_id: String,
    #[serde(rename = "primaryFacilityId")]
    primary_facility_id: String,
    #[serde(rename = "primaryPositionId")]
    primary_position_id: String,
    role: String,
    positions: Vec<VNasPosition>,
    #[serde(rename = "isActive")]
    is_active: bool,
    #[serde(rename = "isObserver")]
    is_observer: bool,
    #[serde(rename = "loginTime")]
    login_time: DateTime<Utc>,
    #[serde(rename = "vatsimData")]
    vatsim_data: VNasVatsimData,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct VNasPosition {
    #[serde(rename = "facilityId")]
    facility_id: String,
    #[serde(rename = "facilityName")]
    facility_name: String,
    #[serde(rename = "positionId")]
    position_id: String,
    #[serde(rename = "positionName")]
    position_name: String,
    #[serde(rename = "positionType")]
    position_type: String,
    #[serde(rename = "radioName")]
    radio_name: String,
    #[serde(rename = "defaultCallsign")]
    default_callsign: String,
    frequency: u32,
    #[serde(rename = "isPrimary")]
    is_primary: bool,
    #[serde(rename = "isActive")]
    is_active: bool,
    #[serde(rename = "eramData")]
    eram_data: Option<VNasEramData>,
    #[serde(rename = "starsData")]
    stars_data: Option<VNasStarsData>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct VNasEramData {
    #[serde(rename = "sectorId")]
    sector_id: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct VNasStarsData {
    subset: u32,
    #[serde(rename = "sectorId")]
    sector_id: String,
    #[serde(rename = "areaId")]
    area_id: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct VNasVatsimData {
    cid: String,
    #[serde(rename = "realName")]
    real_name: String,
    #[serde(rename = "controllerInfo")]
    controller_info: Option<String>,
    #[serde(rename = "userRating")]
    user_rating: String,
    #[serde(rename = "requestedRating")]
    requested_rating: String,
    callsign: String,
    #[serde(rename = "facilityType")]
    facility_type: String,
    #[serde(rename = "primaryFrequency")]
    primary_frequency: u32,
}

/// Query for the current vNAS data.
async fn get_vnas_data() -> Result<VNasData> {
    let resp = GENERAL_HTTP_CLIENT.get(URL).send().await?;
    if !resp.status().is_success() {
        bail!(
            "Got status {} from the vNAS data API",
            resp.status().as_u16()
        );
    }
    let data = resp.json().await?;
    Ok(data)
}

/// Online controllers, suitable for displaying on the site homepage
/// or in Discord.
#[derive(Debug, Serialize)]
pub struct OnlineController {
    pub cid: String,
    pub callsign: String,
    pub name: String,
    pub online_for: String,
    pub frequency: String,
}

/// Use the vNAS data feed to get a list of online controllers.
pub async fn get_online_facility_controllers(config: &Config) -> Result<Vec<OnlineController>> {
    let data = get_vnas_data().await?;
    let now = chrono::Utc::now();
    let mut online: Vec<OnlineController> = Vec::new();

    for controller in data.controllers {
        for position in controller.positions {
            if position.is_primary
                && position_in_facility_airspace(config, &position.default_callsign)
            {
                let online_seconds = (now - controller.login_time).num_seconds() as u32;
                online.push(OnlineController {
                    cid: controller.vatsim_data.cid.clone(),
                    callsign: position.default_callsign.clone(),
                    name: controller.vatsim_data.real_name.clone(),
                    online_for: format!(
                        "{}h{}m",
                        online_seconds / 3600,
                        (online_seconds / 60) % 60
                    ),
                    frequency: format!("{}", position.frequency as f32 / 1_000_000.0),
                });
            }
        }
    }

    Ok(online)
}
