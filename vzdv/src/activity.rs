use crate::sql::{self, Activity, Controller};
use anyhow::Result;
use chrono::{DateTime, Utc};
use itertools::Itertools;
use serde::Serialize;
use sqlx::{Pool, Sqlite};

#[derive(Debug, Serialize)]
pub struct ActivityMonth {
    pub value: u32,
    pub position: Option<u8>,
}

impl From<u32> for ActivityMonth {
    fn from(value: u32) -> Self {
        Self {
            value,
            position: None,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ControllerActivity {
    pub name: String,
    pub ois: String,
    pub cid: u32,
    pub loa_until: Option<DateTime<Utc>>,
    pub rating: i8,
    pub months: Vec<ActivityMonth>,
    pub violation: bool,
}

/// Get activity for all controllers on the roster in the given months.
///
/// The `months` collection should be in the "%Y-%m" format, i.e. "2025-10".
pub async fn get_controller_activity(
    db: &Pool<Sqlite>,
    months: &[String],
) -> Result<Vec<ControllerActivity>> {
    log::info!("months is {months:?}");

    let controllers: Vec<Controller> = sqlx::query_as(sql::GET_ALL_CONTROLLERS_ON_ROSTER)
        .fetch_all(db)
        .await?;
    let activity: Vec<Activity> = sqlx::query_as(sql::GET_ALL_ACTIVITY).fetch_all(db).await?;

    // collect activity into months by controller
    let activity_data: Vec<ControllerActivity> = controllers
        .iter()
        .map(|controller| {
            let this_controller: Vec<_> = activity
                .iter()
                .filter(|a| a.cid == controller.cid)
                .collect();
            let month_data: Vec<ActivityMonth> = (0..months.len())
                .map(|month| {
                    this_controller
                        .iter()
                        .filter(|a| a.month == months[month])
                        .map(|a| a.minutes)
                        .sum::<u32>()
                        .into()
                })
                .collect();
            // 3 hours per quarter are required
            let violation = month_data
                .iter()
                .take(3)
                .map(|month| month.value)
                .sum::<u32>()
                < 180;

            ControllerActivity {
                name: format!("{} {}", controller.first_name, controller.last_name),
                ois: match &controller.operating_initials {
                    Some(ois) => ois.to_owned(),
                    None => String::new(),
                },
                cid: controller.cid,
                loa_until: controller.loa_until,
                rating: controller.rating,
                months: month_data,
                violation,
            }
        })
        .sorted_by(|a, b| Ord::cmp(&a.cid, &b.cid))
        .collect();
    Ok(activity_data)
}
