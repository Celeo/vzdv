//! Backgroudn task for deleting stale ATIS data from the database.

use anyhow::Result;
use chrono::{TimeDelta, Utc};
use log::error;
use sqlx::{Pool, Sqlite};
use vzdv::sql::{self, Atis};

/// Delete stale ATIS data from the database.
pub async fn cleanup(db: &Pool<Sqlite>) -> Result<()> {
    let data: Vec<Atis> = sqlx::query_as(sql::GET_ALL_ATIS_ENTRIES)
        .fetch_all(db)
        .await?;
    let now = Utc::now();
    let expired: Vec<_> = data
        .iter()
        .filter(|e| (now - e.timestamp) >= TimeDelta::hours(1))
        .map(|e| e.id)
        .collect();
    for index in expired {
        if let Err(e) = sqlx::query(sql::DELETE_ATIS_ENTRY)
            .bind(index)
            .execute(db)
            .await
        {
            error!("Could not delete stale ATIS {index}: {e}");
        }
    }
    Ok(())
}
