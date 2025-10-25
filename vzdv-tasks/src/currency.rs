//! Email reminders for currency requirements.

use anyhow::Result;
use chrono::{Days, Months, Utc};
use log::{error, info};
use sqlx::{Pool, Sqlite};
use std::{collections::HashMap, sync::LazyLock};
use vzdv::{
    activity::get_controller_activity,
    config::Config,
    email,
    sql::{self, Controller},
};

/// Months & days the quarters end on.
///
/// Controllers must have currency by these dates to
/// avoid potential roster removal.
static QUARTER_END_DATES: LazyLock<Vec<&str>> =
    LazyLock::new(|| vec!["3/31", "6/30", "9/30", "12/31"]);

/// Potentailly send currency reminder emails.
///
/// Check if the day is 2 weeks or 1 month before the end of a quarter,
/// and send currency reminder emails for controllers who have not met
/// currency requirements in that quater.
///
/// Note that this function calls `.unwrap()` on `chrono` operations,
/// as there's basically zero chance they ever fail.
pub async fn reminders(config: &Config, db: &Pool<Sqlite>) -> Result<()> {
    let today = Utc::now();
    let today_as_str = today.format("%m/%d").to_string();

    let end_of_quarter = {
        let mut date = today;
        loop {
            let as_str = date.format("%m/%d").to_string();
            if QUARTER_END_DATES.contains(&as_str.as_str()) {
                break date;
            }
            date = date.checked_add_days(Days::new(1)).unwrap();
        }
    };

    let one_month_back = end_of_quarter.checked_sub_months(Months::new(1)).unwrap();
    let two_weeks_back = end_of_quarter.checked_sub_days(Days::new(14)).unwrap();
    let should_send_today = today_as_str == one_month_back.format("%m/%d").to_string()
        || today_as_str == two_weeks_back.format("%m/%d").to_string();
    if !should_send_today {
        return Ok(());
    }

    let months = vec![
        end_of_quarter
            .checked_sub_months(Months::new(3))
            .unwrap()
            .format("%Y-%m")
            .to_string(),
        end_of_quarter
            .checked_sub_months(Months::new(2))
            .unwrap()
            .format("%Y-%m")
            .to_string(),
        end_of_quarter.format("%Y-%m").to_string(),
    ];

    let controller_activity = get_controller_activity(db, &months).await?;
    for ca in controller_activity.iter().filter(|ca| ca.violation) {
        let controller: Controller = sqlx::query_as(sql::GET_CONTROLLER_BY_CID)
            .bind(ca.cid)
            .fetch_one(db)
            .await
            .unwrap();
        info!("Sending currency reminder to {} ({})", ca.name, ca.cid);
        if let Err(e) = email::send_mail(
            config,
            db,
            &ca.name,
            &controller.email,
            email::templates::CURRENCY_REQUIRED,
            Some(HashMap::from([
                (email::EmailExtraKeys::CurrencyHours, String::new()),
                (email::EmailExtraKeys::QuarterEnd, String::new()),
            ])),
        )
        .await
        {
            error!(
                "Could not send currency reminder to {} ({}): {e}",
                ca.name, ca.cid
            );
        }
    }

    Ok(())
}
