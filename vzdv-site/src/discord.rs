//! Utilities for the Discord REST API for use by the site,
//! _not_ by the bot itself.

use crate::shared::AppError;
use serde::Deserialize;
use std::collections::HashMap;
use vzdv::{GENERAL_HTTP_CLIENT, config::Config};

// In each of these structs, there are other fields that are returned by their respective
// API endpoints, but these are the only fields that are actually needed.

/// Response from the Discord OAuth exchange URL.
#[derive(Deserialize)]
pub struct DiscordAccessToken {
    pub token_type: String,
    pub access_token: String,
}

#[derive(Deserialize)]
struct DiscordUserInfo {
    user: DiscordUserInfoUser,
}

#[derive(Deserialize)]
struct DiscordUserInfoUser {
    id: String,
}

/// Generate the URL to navigate users to in order to start the Discord OAuth flow.
pub fn get_oauth_link(config: &Config) -> String {
    format!(
        "https://discord.com/oauth2/authorize?client_id={}&response_type=code&redirect_uri={}&scope=identify",
        &config.discord.auth.client_id, &config.discord.auth.redirect_uri,
    )
}

/// Exchange the code received from the Discord OAuth callback for an access token.
pub async fn code_to_token(code: &str, config: &Config) -> Result<DiscordAccessToken, AppError> {
    let data = HashMap::from([
        ("grant_type", "authorization_code"),
        ("redirect_uri", &config.discord.auth.redirect_uri),
        ("code", code),
    ]);
    let resp = GENERAL_HTTP_CLIENT
        .post("https://discord.com/api/v10/oauth2/token")
        .basic_auth(
            &config.discord.auth.client_id,
            Some(&config.discord.auth.client_secret),
        )
        .form(&data)
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(AppError::HttpResponse(
            "Discord OAuth token exchange",
            resp.status().as_u16(),
        ));
    }
    let data = resp.json().await?;
    Ok(data)
}

/// Use a Discord OAuth access token to get the user ID for the user it represents.
pub async fn get_token_user_id(access_token: &DiscordAccessToken) -> Result<String, AppError> {
    let resp = GENERAL_HTTP_CLIENT
        .get("https://discord.com/api/oauth2/@me")
        .header(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&format!(
                "{} {}",
                access_token.token_type, access_token.access_token
            ))
            .unwrap(),
        )
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(AppError::HttpResponse(
            "Discord OAuth user info lookup",
            resp.status().as_u16(),
        ));
    }
    let data: DiscordUserInfo = resp.json().await?;
    Ok(data.user.id)
}
