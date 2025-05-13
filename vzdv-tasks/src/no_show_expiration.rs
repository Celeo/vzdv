use anyhow::{Result, anyhow};
use chrono::{Months, Utc};
use log::info;
use sqlx::{Pool, Sqlite};
use vzdv::sql::{self, NoShow};

pub async fn check_expired(db: &Pool<Sqlite>) -> Result<()> {
    let no_shows: Vec<NoShow> = sqlx::query_as(sql::GET_ALL_NO_SHOW).fetch_all(db).await?;
    let now = Utc::now();

    for entry in no_shows {
        let has_expired = entry
            .created_date
            .checked_add_months(Months::new(6))
            .ok_or_else(|| anyhow!("could not compute no-show expiration date"))?
            < now;
        if has_expired {
            info!("No-show {} has expired", entry.id,);
            sqlx::query(sql::DELETE_NO_SHOW_ENTRY)
                .bind(entry.id)
                .execute(db)
                .await?;
            info!(
                "No-show {} ({}) for {} from {} created {} deleted",
                entry.id,
                entry.entry_type,
                entry.cid,
                entry.reported_by,
                entry.created_date.to_rfc3339()
            );
        }
    }

    Ok(())
}
