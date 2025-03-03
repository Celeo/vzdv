//! Wrappers around the `vzdv::vatusa` module using `AppError`.

use crate::AppError;
use chrono::{DateTime, Utc};
use vzdv::vatusa::{self, RatingHistory, RosterMember, TransferChecklist};

pub use vzdv::vatusa::{NewTrainingRecord, TrainingRecord};

/// Get the controller's public information.
///
/// Supply a VATUSA API key to get private information.
pub async fn get_controller_info(
    cid: u32,
    api_key: Option<&str>,
) -> Result<RosterMember, AppError> {
    let data = vatusa::get_controller_info(cid, api_key)
        .await
        .map_err(AppError::VatusaApi)?;
    Ok(data)
}

/// Get the controller's training records.
pub async fn get_training_records(
    cid: u32,
    api_key: &str,
) -> Result<Vec<TrainingRecord>, AppError> {
    let data = vatusa::get_training_records(cid, api_key)
        .await
        .map_err(AppError::VatusaApi)?;
    Ok(data)
}

/// Add a new training record to the controller's VATUSA record.
pub async fn save_training_record(
    api_key: &str,
    cid: u32,
    data: &NewTrainingRecord,
) -> Result<(), AppError> {
    vatusa::save_training_record(api_key, cid, data)
        .await
        .map_err(AppError::VatusaApi)?;
    Ok(())
}

/// Get the controller's transfer checklist information.
pub async fn transfer_checklist(cid: u32, api_key: &str) -> Result<TransferChecklist, AppError> {
    let data = vatusa::transfer_checklist(cid, api_key)
        .await
        .map_err(AppError::VatusaApi)?;
    Ok(data)
}

/// Report a new solo cert to VATUSA.
pub async fn report_solo_cert(
    cid: u32,
    position: &str,
    expiration: DateTime<Utc>,
    api_key: &str,
) -> Result<(), AppError> {
    vatusa::report_solo_cert(cid, position, expiration, api_key)
        .await
        .map_err(AppError::VatusaApi)?;
    Ok(())
}

/// Delete a solo cert from VATUSA.
pub async fn delete_solo_cert(cid: u32, position: &str, api_key: &str) -> Result<(), AppError> {
    vatusa::delete_solo_cert(cid, position, api_key)
        .await
        .map_err(AppError::VatusaApi)?;
    Ok(())
}

/// Get a controller's rating history.
pub async fn get_controller_rating_history(
    cid: u32,
    api_key: &str,
) -> Result<Vec<RatingHistory>, AppError> {
    let data = vatusa::get_controller_rating_history(cid, api_key)
        .await
        .map_err(AppError::VatusaApi)?;
    Ok(data)
}

/// Remove a home controller from the roster.
pub async fn remove_home_controller(
    cid: u32,
    by: &str,
    reason: &str,
    api_key: &str,
) -> Result<(), AppError> {
    vatusa::remove_home_controller(cid, by, reason, api_key)
        .await
        .map_err(AppError::VatusaApi)?;
    Ok(())
}

/// Remove a visiting controller from the roster.
pub async fn remove_visiting_controller(
    cid: u32,
    reason: &str,
    api_key: &str,
) -> Result<(), AppError> {
    vatusa::remove_visiting_controller(cid, reason, api_key)
        .await
        .map_err(AppError::VatusaApi)?;
    Ok(())
}

/// Add a visiting controller to the roster.
pub async fn add_visiting_controller(cid: u32, api_key: &str) -> Result<(), AppError> {
    vatusa::add_visiting_controller(cid, api_key)
        .await
        .map_err(AppError::VatusaApi)?;
    Ok(())
}
