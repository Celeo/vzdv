use crate::{
    flashed_messages::{self, MessageLevel},
    load_templates,
    shared::{strip_some_tags, AppError, AppState, UserInfo, SESSION_USER_INFO_KEY},
};
use axum::{
    extract::{Query, State},
    response::{Html, IntoResponse, Redirect, Response},
    routing::get,
    Json, Router,
};
use chrono::NaiveDateTime;
use minijinja::context;
use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, collections::HashMap, sync::Arc};
use tower_sessions::Session;
use vzdv::{
    controller_can_see,
    sql::{self, Certification, Controller},
    vatusa::{self, TrainingRecord},
    ControllerRating,
};

#[derive(Debug, Serialize, Deserialize)]
struct CalendarEventExtra {
    cid: u32,
    schedule: Option<u32>,
    taken: bool,
    taken_by: Option<u32>,
    positions: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CalendarEvent {
    id: String,
    title: String,
    start: String,
    end: String,
    editable: bool,
    #[serde(rename = "backgroundColor")]
    background_color: String,
    #[serde(rename = "textColor")]
    text_color: String,
    #[serde(rename = "extendedProps")]
    extended_props: CalendarEventExtra,
}

#[derive(Debug, Serialize)]
struct CertForTmpl {
    name: String,
    style: &'static str,
    order: usize,
}

/// Construct a list of certs for the overall controller's progress list.
fn progress_part_certs(
    starting_index: usize,
    controller_certs: &[String],
    config_certs: &[String],
    prefix: &str,
    out_of_reach: bool,
) -> (Vec<CertForTmpl>, bool) {
    let mut out_of_reach = out_of_reach;
    let mut ret = Vec::new();
    for (index, cert) in config_certs
        .iter()
        .filter(|name| name.starts_with(prefix))
        .enumerate()
    {
        let has = controller_certs.contains(cert);
        if has {
            ret.push(CertForTmpl {
                name: cert.to_string(),
                style: "success",
                order: starting_index + index + 1,
            });
        } else if out_of_reach {
            ret.push(CertForTmpl {
                name: cert.to_string(),
                style: "light",
                order: starting_index + index + 1,
            });
        } else {
            out_of_reach = true;
            ret.push(CertForTmpl {
                name: cert.to_string(),
                style: "warning",
                order: starting_index + index + 1,
            });
        }
    }
    (ret, out_of_reach)
}

/// Construct a list of ratings and cert levels that reflect the controller's learning progress.
///
/// The first rating or cert that they don't have will be marked specifically, denoting
/// that step as next in the controller's learning journey. All following non-held ratings/certs
/// will be marked as out of reach.
fn progress_list(
    rating: i8,
    controller_certs: &[String],
    config_certs: &[String],
) -> Vec<CertForTmpl> {
    let mut ret = Vec::new();
    let mut out_of_reach = false;
    // ~~~ ground
    ret.push(CertForTmpl {
        name: String::from("S1"),
        style: match rating.cmp(&ControllerRating::S1.as_id()) {
            Ordering::Less => "warning",
            _ => "success",
        },
        order: 10,
    });
    let ground_certs = progress_part_certs(10, controller_certs, config_certs, "GC", out_of_reach);
    out_of_reach = ground_certs.1;
    ret.extend(ground_certs.0);
    // ~~~ tower
    if rating >= ControllerRating::S2.as_id() {
        ret.push(CertForTmpl {
            name: String::from("S2"),
            style: "success",
            order: 20,
        });
    } else if out_of_reach {
        ret.push(CertForTmpl {
            name: String::from("S2"),
            style: "light",
            order: 20,
        });
    } else {
        out_of_reach = true;
        ret.push(CertForTmpl {
            name: String::from("S2"),
            style: "warning",
            order: 20,
        });
    }
    let tower_certs = progress_part_certs(20, controller_certs, config_certs, "LC", out_of_reach);
    out_of_reach = tower_certs.1;
    ret.extend(tower_certs.0);
    // ~~~ approach
    if rating >= ControllerRating::S3.as_id() {
        ret.push(CertForTmpl {
            name: String::from("S3"),
            style: "success",
            order: 30,
        });
    } else if out_of_reach {
        ret.push(CertForTmpl {
            name: String::from("S3"),
            style: "light",
            order: 30,
        });
    } else {
        out_of_reach = true;
        ret.push(CertForTmpl {
            name: String::from("S3"),
            style: "warning",
            order: 30,
        });
    }
    let approach_certs =
        progress_part_certs(30, controller_certs, config_certs, "APP", out_of_reach);
    out_of_reach = approach_certs.1;
    ret.extend(approach_certs.0);
    // ~~~ center
    if rating >= ControllerRating::C1.as_id() {
        ret.push(CertForTmpl {
            name: String::from("C1"),
            style: "success",
            order: 40,
        });
    } else if out_of_reach {
        ret.push(CertForTmpl {
            name: String::from("C1"),
            style: "light",
            order: 40,
        });
    } else {
        ret.push(CertForTmpl {
            name: String::from("C1"),
            style: "warning",
            order: 40,
        });
    }
    // done
    ret.sort_by(|a, b| a.order.cmp(&b.order));
    ret
}

/// Training center homepage.
async fn page_training_home(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Response, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    let user_info = match user_info {
        Some(ui) => ui,
        None => {
            flashed_messages::push_flashed_message(
                session,
                MessageLevel::Error,
                "You must be logged in to view this page",
            )
            .await?;
            return Ok(Redirect::to("/").into_response());
        }
    };

    let controller: Controller = sqlx::query_as(sql::GET_CONTROLLER_BY_CID)
        .bind(user_info.cid)
        .fetch_one(&state.db)
        .await?;

    if !controller.is_on_roster {
        flashed_messages::push_flashed_message(
            session,
            MessageLevel::Error,
            "Training is for home and visiting controllers",
        )
        .await?;
        return Ok(Redirect::to("/").into_response());
    }

    let controller_certs: Vec<Certification> = sqlx::query_as(sql::GET_ALL_CERTIFICATIONS_FOR)
        .bind(user_info.cid)
        .fetch_all(&state.db)
        .await?;
    let progress_strip = progress_list(
        controller.rating,
        &controller_certs
            .iter()
            .filter(|c| c.value == "trained")
            .map(|c| c.name.clone())
            .collect::<Vec<_>>(),
        &state.config.training.certifications,
    );

    let env = load_templates().unwrap();
    let template = env.get_template("training/home.jinja")?;
    let flashed_messages = flashed_messages::drain_flashed_messages(session).await?;
    let rendered = template.render(context! {
        user_info,
        flashed_messages,
        progress_strip,
        controller,
        is_training_staff => controller_can_see(&Some(controller), vzdv::PermissionsGroup::TrainingTeam),
    })?;
    Ok(Html(rendered).into_response())
}
/// Retrieve and show the user their training records from VATUSA.
async fn page_training_notes(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Response, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    let user_info = match user_info {
        Some(info) => info,
        None => return Ok(Redirect::to("/").into_response()),
    };
    let all_training_records =
        vatusa::get_training_records(user_info.cid, &state.config.vatsim.vatusa_api_key)
            .await
            .map_err(|e| {
                AppError::GenericFallback("getting VATUSA training records by controller", e)
            })?;
    let mut training_records: Vec<_> = all_training_records
        .iter()
        .filter(|record| record.facility_id == "ZDV")
        .map(|record| {
            let record = record.clone();
            TrainingRecord {
                notes: strip_some_tags(&record.notes).replace("\n", "<br>"),
                ..record
            }
        })
        .collect();

    // sort by session_date in descending order (newest first)
    training_records.sort_by(|a, b| {
        let date_a = NaiveDateTime::parse_from_str(&a.session_date, "%Y-%m-%d %H:%M:%S")
            .unwrap_or_else(|_| NaiveDateTime::default());
        let date_b = NaiveDateTime::parse_from_str(&b.session_date, "%Y-%m-%d %H:%M:%S")
            .unwrap_or_else(|_| NaiveDateTime::default());
        date_b.cmp(&date_a) // sort newest first
    });

    let template = state.templates.get_template("training/self_notes.jinja")?;
    let rendered = template.render(context! { user_info, training_records })?;
    Ok(Html(rendered).into_response())
}

/// Return a set of events for the calendar UI that the user has access to.
///
/// Depending on the user's role, they will have additional access:
///     - Most users can only see available sessions
///     - A trainer can see all of their sessions
///     - The TA can see all sessions, available and not
///
/// All sessions go from today to 60 days in the future. The calendar app will
/// send a date range in the query string, but I'll probably just ignore that.
/// Could get the user's timezone offset from those, though.
async fn api_get_training_sessions(
    State(_state): State<Arc<AppState>>,
    session: Session,
    Query(_params): Query<HashMap<String, String>>,
) -> Result<Json<Vec<CalendarEvent>>, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    let _user_info = match user_info {
        Some(info) => info,
        None => return Ok(Json(Vec::new())),
    };

    /*
     * Color legend:
     *  - Red = self as student
     *  - Green = available
     *  - Gray = unavailable
     *  - Blue = self as trainer, open
     *  - Yellow = self as trainer, taken
     */

    // TODO
    Ok(Json(vec![
        CalendarEvent {
            id: String::from("19231"),
            title: String::from("S3 conceptual review"),
            start: String::from("2024-11-05T10:00:00"),
            end: String::from("2024-11-05T11:00:00"),
            editable: false,
            background_color: String::from("red"),
            text_color: String::from("white"),
            extended_props: CalendarEventExtra {
                cid: 811918,
                schedule: None,
                taken: true,
                taken_by: Some(10000005),
                positions: vec![],
            },
        },
        CalendarEvent {
            id: String::from("24981"),
            title: String::from("Trainer Availability: GC Un/T2/T1 | LC Un/T2"),
            start: String::from("2024-11-04T22:00:00"),
            end: String::from("2024-11-05T02:00:00"),
            editable: false,
            background_color: String::from("green"),
            text_color: String::from("black"),
            extended_props: CalendarEventExtra {
                cid: 811918,
                schedule: None,
                taken: false,
                taken_by: None,
                positions: vec![],
            },
        },
        CalendarEvent {
            id: String::from("24981"),
            title: String::from("Trainer Availability: GC Un/T2/T1 | LC Un/T2"),
            start: String::from("2024-11-06T14:00:00"),
            end: String::from("2024-11-06T20:00:00"),
            editable: false,
            background_color: String::from("green"),
            text_color: String::from("black"),
            extended_props: CalendarEventExtra {
                cid: 811918,
                schedule: None,
                taken: false,
                taken_by: None,
                positions: vec![],
            },
        },
        CalendarEvent {
            id: String::from("24981"),
            title: String::from("Trainer Availability: GC Un/T2/T1 | LC Un/T2"),
            start: String::from("2024-11-08T22:00:00"),
            end: String::from("2024-11-09T02:00:00"),
            editable: false,
            background_color: String::from("gray"),
            text_color: String::from("black"),
            extended_props: CalendarEventExtra {
                cid: 811918,
                schedule: None,
                taken: true,
                taken_by: Some(10000003),
                positions: vec![],
            },
        },
        CalendarEvent {
            id: String::from("24981"),
            title: String::from("Trainer Availability: GC Un/T2/T1 | LC Un/T2"),
            start: String::from("2024-11-08T22:00:00"),
            end: String::from("2024-11-09T02:00:00"),
            editable: false,
            background_color: String::from("green"),
            text_color: String::from("black"),
            extended_props: CalendarEventExtra {
                cid: 811918,
                schedule: None,
                taken: false,
                taken_by: None,
                positions: vec![],
            },
        },
    ]))
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/training", get(page_training_home))
        .route("/training/my_notes", get(page_training_notes))
        .route("/training/sessions", get(api_get_training_sessions))
}

#[cfg(test)]
mod tests {
    use super::progress_list;
    use vzdv::ControllerRating;

    #[test]
    fn test_progress_list_s1() {
        let rating = ControllerRating::S1.as_id();
        let certs = progress_list(rating, &[], &[]);

        assert_eq!(certs.len(), 4);
        assert_eq!(certs.get(0).unwrap().style, "success");
        assert_eq!(certs.get(1).unwrap().style, "warning");
        assert_eq!(certs.get(2).unwrap().style, "light");
        assert_eq!(certs.get(3).unwrap().style, "light");
    }

    #[test]
    fn test_progress_list_s3() {
        let rating = ControllerRating::S3.as_id();
        let certs = progress_list(rating, &[], &[]);

        assert_eq!(certs.len(), 4);
        assert_eq!(certs.get(0).unwrap().style, "success");
        assert_eq!(certs.get(1).unwrap().style, "success");
        assert_eq!(certs.get(2).unwrap().style, "success");
        assert_eq!(certs.get(3).unwrap().style, "warning");
    }

    #[test]
    fn test_progress_list_s2_missing_ground_extra() {
        let rating = ControllerRating::S2.as_id();
        let certs = progress_list(
            rating,
            &[String::from("GC EGE T2")],
            &[String::from("GC EGE T2"), String::from("GC ASE T2")],
        );

        assert_eq!(certs.len(), 6);
        assert_eq!(certs.get(0).unwrap().style, "success");
        assert_eq!(certs.get(1).unwrap().style, "success");
        assert_eq!(certs.get(2).unwrap().style, "warning");
        assert_eq!(certs.get(3).unwrap().style, "success");
        assert_eq!(certs.get(4).unwrap().style, "light");
        assert_eq!(certs.get(5).unwrap().style, "light");
    }

    #[test]
    fn test_progress_fresh_new_c1() {
        let rating = ControllerRating::C1.as_id();
        let certs = progress_list(
            rating,
            &[],
            &[
                String::from("GC T2 EGE"),
                String::from("GC T2 ASE"),
                String::from("GC T1"),
                String::from("LC T2 EGE"),
                String::from("LC T2 ASE"),
                String::from("LC T1"),
                String::from("APP T2 GJT"),
                String::from("APP T2 ASE"),
                String::from("APP T1"),
            ],
        );

        assert_eq!(certs.len(), 13);
        #[rustfmt::skip]
        assert_eq!(certs.iter().map(|c| c.style).collect::<Vec<_>>(), vec![
            "success", // S1
            "warning", // first training to do
            "light", // first out of reach; all other non-rating certs should be "light"
            "light",
            "success", // S2
            "light",
            "light",
            "light",
            "success", // S3
            "light",
            "light",
            "light",
            "success" // C1
        ]);
    }
}
