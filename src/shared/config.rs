use serde::{Deserialize, Serialize};

/// Default place to look for the config file.
pub const DEFAULT_CONFIG_FILE_NAME: &str = "site_config.toml";

/// App configuration.
#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub database: ConfigDatabase,
    pub vatsim: ConfigVatsim,
    pub airports: ConfigAirports,
    pub stats: ConfigStats,
    pub webhooks: ConfigWebhooks,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ConfigDatabase {
    pub file: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ConfigVatsim {
    pub oauth_client_id: String,
    pub oauth_client_secret: String,
    pub oauth_client_callback_url: String,
    pub vatusa_facility_code: String,
    pub vatusa_api_key: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ConfigAirports {
    pub all: Vec<Airport>,
    pub weather_for: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Airport {
    pub code: String,
    pub name: String,
    pub location: String,
    pub towered: bool,
    pub class: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ConfigStats {
    pub position_prefixes: Vec<String>,
    pub position_suffixes: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ConfigWebhooks {
    pub staffing_request: String,
    pub feedback: String,
}
