//! Endpoints for getting information on the facility.

use crate::{
    flashed_messages,
    shared::{AppError, AppState, UserInfo, SESSION_USER_INFO_KEY},
};
use axum::{
    extract::State,
    response::{Html, Redirect},
    routing::get,
    Form, Router,
};
use chrono::{DateTime, Months, Utc};
use itertools::Itertools;
use log::warn;
use minijinja::{context, Environment};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use tower_sessions::Session;
use vzdv::{
    config::Config,
    determine_staff_positions,
    sql::{self, Activity, Certification, Controller, Resource, VisitorRequest},
    vatusa, ControllerRating,
};

#[derive(Debug, Serialize)]
struct StaffPosition {
    short: &'static str,
    name: &'static str,
    order: u8,
    controllers: Vec<Controller>,
    email: Option<String>,
    description: &'static str,
}

fn generate_staff_outline(config: &Config) -> HashMap<&'static str, StaffPosition> {
    let email_domain = &config.staff.email_domain;
    HashMap::from([
        ("ATM", StaffPosition {
            short: "ATM",
            name: "Air Traffic Manager",
            order: 1,
            controllers: Vec::new(),
            email: Some(format!("atm@{email_domain}")),
            description: "Responsible for the macro-management of the facility. Oversees day-to-day operations and ensures that the facility is running smoothly.",
        }),
        ("DATM", StaffPosition {
            short: "DATM",
            name: "Deputy Air Traffic Manager",
            order: 2,
            controllers: Vec::new(),
            email: Some(format!("datm@{email_domain}")),
            description: "Assists the Air Traffic Manager with the management of the facility. Acts as the Air Traffic Manager in their absence.",
        }),
        ("TA", StaffPosition {
            short: "TA",
            name: "Training Administrator",
            order: 3,
            controllers: Vec::new(),
            email: Some(format!("ta@{email_domain}")),
            description: "Responsible for overseeing and management of the facility's training program and staff.",
        }),
        ("FE", StaffPosition {
            short: "FE",
            name: "Facility Engineer",
            order: 4,
            controllers: Vec::new(),
            email: Some(format!("fe@{email_domain}")),
            description: "Responsible for the creation of sector files, radar client files, and other facility resources.",
        }),
        ("EC", StaffPosition {
            short: "EC",
            name: "Events Coordinator",
            order: 5,
            controllers: Vec::new(),
            email: Some(format!("ec@{email_domain}")),
            description: "Responsible for the planning, coordination and advertisement of facility events with neighboring facilities, virtual airlines, VATUSA, and VATSIM.",
        }),
        ("WM", StaffPosition {
            short: "WM",
            name: "Webmaster",
            order: 6,
            controllers: Vec::new(),
            email: Some(format!("wm@{email_domain}")),
            description: "Responsible for the management of the facility's website and technical infrastructure.",
        }),
        ("INS", StaffPosition {
            short: "INS",
            name: "Instructor",
            order: 7,
            controllers: Vec::new(),
            email: None,
            description: "Under direction of the Training Administrator, leads training and handles OTS Examinations.",
        }),
        ("MTR", StaffPosition {
            short: "MTR",
            name: "Mentor",
            order: 8,
            controllers: Vec::new(),
            email: None,
            description: "Under direction of the Training Administrator, helps train students and prepare them for OTS Examinations.",
        }),
        ("AFE", StaffPosition {
            short: "AFE",
            name: "Assistant Facility Engineer",
            order: 9,
            controllers: Vec::new(),
            email: None,
            description: "Assists the Facility Engineer.",
        }),
        ("AEC", StaffPosition {
            short: "AEC",
            name: "Assistant Events Coordinator",
            order: 10,
            controllers: Vec::new(),
            email: None,
            description: "Assists the Events Coordinator.",
        }),
        ("AWM", StaffPosition {
            short: "AWM",
            name: "Assistant Webmaster",
            order: 11,
            controllers: Vec::new(),
            email: None,
            description: "Assists the Webmaster.",
        }),
    ])
}

#[derive(Debug, Serialize)]
struct ControllerWithCerts<'a> {
    cid: u32,
    first_name: &'a str,
    last_name: &'a str,
    operating_initials: &'a str,
    rating: &'static str,
    is_home: bool,
    roles: String,
    certs: Vec<Certification>,
    loa_until: Option<DateTime<Utc>>,
}

/// View the full roster.
async fn page_roster(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Html<String>, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    let controllers: Vec<Controller> = sqlx::query_as(sql::GET_ALL_CONTROLLERS_ON_ROSTER)
        .fetch_all(&state.db)
        .await?;
    let certifications: Vec<Certification> = sqlx::query_as(sql::GET_ALL_CERTIFICATIONS)
        .fetch_all(&state.db)
        .await?;

    let certification_order = &state.config.training.certifications;
    let cert_order_map: HashMap<&String, usize> = certification_order
        .into_iter()
        .enumerate()
        .map(|(index, cert)| (cert, index))
        .collect();

    let controllers_with_certs: Vec<_> = controllers
        .iter()
        .map(|controller| {
            let operating_initials = match &controller.operating_initials {
                Some(s) => s,
                None => "",
            };
            let roles = determine_staff_positions(controller).join(", ");
            let mut certs = certifications
                .iter()
                .filter(|cert| cert.cid == controller.cid)
                .cloned()
                .collect::<Vec<_>>();

            // Sort certifications based on the order in the TOML file
            certs.sort_by_key(|cert| cert_order_map.get(&cert.name).cloned().unwrap_or(usize::MAX));

            ControllerWithCerts {
                cid: controller.cid,
                first_name: &controller.first_name,
                last_name: &controller.last_name,
                operating_initials,
                rating: ControllerRating::try_from(controller.rating)
                    .map(|r| r.as_str())
                    .unwrap_or(""),
                is_home: controller.home_facility == "ZDV",
                roles,
                certs,
                loa_until: controller.loa_until,
            }
        })
        .sorted_by(|a, b| Ord::cmp(&a.cid, &b.cid))
        .collect();

    let flashed_messages = flashed_messages::drain_flashed_messages(session).await?;
    let template = state.templates.get_template("facility/roster")?;
    let rendered = template.render(context! {
       user_info,
       controllers => controllers_with_certs,
       flashed_messages
    })?;
    Ok(Html(rendered))
}

/// View the facility's staff.
async fn page_staff(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Html<String>, AppError> {
    let mut staff_map = generate_staff_outline(&state.config);
    let controllers: Vec<Controller> = sqlx::query_as(sql::GET_ALL_CONTROLLERS)
        .fetch_all(&state.db)
        .await?;
    for controller in &controllers {
        let roles = determine_staff_positions(controller);
        for role in roles {
            if let Some(staff_pos) = staff_map.get_mut(role.as_str()) {
                staff_pos.controllers.push(controller.clone());
            } else {
                warn!("No staff role found for: {role}");
            }
        }
    }

    let staff: Vec<_> = staff_map
        .values()
        .sorted_by(|a, b| Ord::cmp(&a.order, &b.order))
        .collect();

    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    let template = state.templates.get_template("facility/staff")?;
    let rendered = template.render(context! { user_info, staff })?;
    Ok(Html(rendered))
}

/// View all controller's recent (summarized) controlling activity.
async fn page_activity(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Html<String>, AppError> {
    #[derive(Debug, Serialize)]
    struct ActivityMonth {
        value: u32,
        position: Option<u8>,
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
    struct ControllerActivity {
        name: String,
        ois: String,
        cid: u32,
        loa_until: Option<DateTime<Utc>>,
        rating: i8,
        months: Vec<ActivityMonth>,
        violation: bool,
    }

    // this could be a join, but oh well
    let controllers: Vec<Controller> = sqlx::query_as(sql::GET_ALL_CONTROLLERS_ON_ROSTER)
        .fetch_all(&state.db)
        .await?;
    let activity: Vec<Activity> = sqlx::query_as(sql::GET_ALL_ACTIVITY)
        .fetch_all(&state.db)
        .await?;

    // time ranges
    let now = Utc::now();
    let months: [String; 5] = [
        now.format("%Y-%m").to_string(),
        now.checked_sub_months(Months::new(1))
            .unwrap()
            .format("%Y-%m")
            .to_string(),
        now.checked_sub_months(Months::new(2))
            .unwrap()
            .format("%Y-%m")
            .to_string(),
        now.checked_sub_months(Months::new(3))
            .unwrap()
            .format("%Y-%m")
            .to_string(),
        now.checked_sub_months(Months::new(4))
            .unwrap()
            .format("%Y-%m")
            .to_string(),
    ];

    // collect activity into months by controller
    let mut activity_data: Vec<ControllerActivity> = controllers
        .iter()
        .map(|controller| {
            let this_controller: Vec<_> = activity
                .iter()
                .filter(|a| a.cid == controller.cid)
                .collect();
            let months: Vec<ActivityMonth> = (0..=4)
                .map(|month| {
                    this_controller
                        .iter()
                        .filter(|a| a.month == months[month])
                        .map(|a| a.minutes)
                        .sum::<u32>()
                        .into()
                })
                .collect();
            let violation = months.iter().take(3).map(|month| month.value).sum::<u32>() < 180; // 3 hours in a quarter

            ControllerActivity {
                name: format!("{} {}", controller.first_name, controller.last_name),
                ois: match &controller.operating_initials {
                    Some(ois) => ois.to_owned(),
                    None => String::new(),
                },
                cid: controller.cid,
                loa_until: controller.loa_until,
                rating: controller.rating,
                months,
                violation,
            }
        })
        .sorted_by(|a, b| Ord::cmp(&a.cid, &b.cid))
        .collect();

    // top 3 controllers for each month
    for month in 0..=4 {
        activity_data
            .iter()
            .enumerate()
            .map(|(index, data)| (index, data.months[month].value))
            .sorted_by(|a, b| Ord::cmp(&b.1, &a.1))
            .map(|(index, _data)| index)
            .take(3)
            .enumerate()
            .for_each(|(rank, controller_index)| {
                activity_data[controller_index].months[month].position = Some(rank as u8);
            });
    }

    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    let template = state.templates.get_template("facility/activity")?;
    let rendered = template.render(context! { user_info, activity_data })?;
    Ok(Html(rendered))
}

/// View files uploaded to the site.
async fn page_resources(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Html<String>, AppError> {
    let resources: Vec<Resource> = sqlx::query_as(sql::GET_ALL_RESOURCES)
        .fetch_all(&state.db)
        .await?;
    let resources: Vec<_> = resources
        .iter()
        .sorted_by(|a, b| a.name.cmp(&b.name))
        .collect();

    let categories: Vec<_> = resources
        .iter()
        .map(|r| &r.category)
        .collect::<HashSet<_>>()
        .into_iter()
        .sorted()
        .collect();
    let categories: Vec<_> = state
        .config
        .database
        .resource_category_ordering
        .iter()
        .filter(|category| categories.contains(category))
        .collect();

    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    let template = state.templates.get_template("facility/resources")?;
    let rendered = template.render(context! { user_info, resources, categories })?;
    Ok(Html(rendered))
}

/// Check visitor requirements and submit an application.
async fn page_visitor_application(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Html<String>, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    let controller: Option<Controller> = match user_info {
        Some(ref info) => {
            let controller: Option<Controller> = sqlx::query_as(sql::GET_CONTROLLER_BY_CID)
                .bind(info.cid)
                .fetch_optional(&state.db)
                .await?;
            controller
        }
        None => None,
    };
    let is_visiting = controller
        .as_ref()
        .map(|c| c.is_on_roster)
        .unwrap_or_default();
    let flashed_messages = flashed_messages::drain_flashed_messages(session).await?;
    let template = state
        .templates
        .get_template("facility/visitor_application")?;
    let rendered =
        template.render(context! { user_info, flashed_messages, controller, is_visiting })?;
    Ok(Html(rendered))
}

/// Check visitor eligibility and return either a form or an error message.
async fn page_visitor_application_form(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Html<String>, AppError> {
    let user_info: UserInfo = match session.get(SESSION_USER_INFO_KEY).await? {
        Some(user_info) => user_info,
        // a little lazy, but no one should see this
        None => return Ok(Html(String::from("Must be logged in"))),
    };
    // check pending request
    let pending_request: Option<VisitorRequest> = sqlx::query_as(sql::GET_PENDING_VISITOR_REQ_FOR)
        .bind(user_info.cid)
        .fetch_optional(&state.db)
        .await?;
    // check rating
    let controller_info = match vatusa::get_controller_info(user_info.cid, None).await {
        Ok(info) => Some(info),
        Err(e) => {
            warn!("{e}");
            None
        }
    };
    // check VATUSA checklist
    let checklist = match vatusa::transfer_checklist(
        &state.config.vatsim.vatusa_api_key,
        user_info.cid,
    )
    .await
    {
        Ok(checklist) => Some(checklist),
        Err(e) => {
            warn!("{e}");
            None
        }
    };

    let template = state
        .templates
        .get_template("facility/visitor_application_form")?;
    let rendered =
        template.render(context! { user_info, pending_request, controller_info, checklist })?;
    Ok(Html(rendered))
}

#[derive(Debug, Deserialize)]
struct VisitorApplicationForm {
    rating: u8,
    facility: String,
}

/// Submit the request to join as a visitor.
async fn page_visitor_application_form_submit(
    State(state): State<Arc<AppState>>,
    session: Session,
    Form(application_form): Form<VisitorApplicationForm>,
) -> Result<Redirect, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    if let Some(user_info) = user_info {
        sqlx::query(sql::INSERT_INTO_VISITOR_REQ)
            .bind(user_info.cid)
            .bind(&user_info.first_name)
            .bind(&user_info.last_name)
            .bind(application_form.facility)
            .bind(application_form.rating)
            .bind(Utc::now())
            .execute(&state.db)
            .await?;
        flashed_messages::push_flashed_message(
            session,
            flashed_messages::MessageLevel::Success,
            "Request submitted, thank you!",
        )
        .await?;
    } else {
        flashed_messages::push_flashed_message(
            session,
            flashed_messages::MessageLevel::Error,
            "You must be logged in to submit a visitor request.",
        )
        .await?;
    }
    Ok(Redirect::to("/facility/visitor_application"))
}

pub fn router(templates: &mut Environment) -> Router<Arc<AppState>> {
    templates
        .add_template(
            "facility/roster",
            include_str!("../../templates/facility/roster.jinja"),
        )
        .unwrap();
    templates
        .add_template(
            "facility/staff",
            include_str!("../../templates/facility/staff.jinja"),
        )
        .unwrap();
    templates
        .add_template(
            "facility/activity",
            include_str!("../../templates/facility/activity.jinja"),
        )
        .unwrap();
    templates
        .add_template(
            "facility/resources",
            include_str!("../../templates/facility/resources.jinja"),
        )
        .unwrap();
    templates
        .add_template(
            "facility/visitor_application",
            include_str!("../../templates/facility/visitor_application.jinja"),
        )
        .unwrap();
    templates
        .add_template(
            "facility/visitor_application_form",
            include_str!("../../templates/facility/visitor_application_form.jinja"),
        )
        .unwrap();
    templates.add_filter("minutes_to_hm", |total_minutes: u32| {
        let hours = total_minutes / 60;
        let minutes = total_minutes % 60;
        if hours > 0 || minutes > 0 {
            format!("{hours}h{minutes}m")
        } else {
            String::new()
        }
    });
    templates.add_filter("simple_date", |date: String| {
        chrono::DateTime::parse_from_rfc3339(&date)
            .unwrap()
            .format("%m/%d/%Y")
            .to_string()
    });

    Router::new()
        .route("/facility/roster", get(page_roster))
        .route("/facility/staff", get(page_staff))
        .route("/facility/activity", get(page_activity))
        .route("/facility/resources", get(page_resources))
        .route(
            "/facility/visitor_application",
            get(page_visitor_application),
        )
        .route(
            "/facility/visitor_application/form",
            get(page_visitor_application_form).post(page_visitor_application_form_submit),
        )
}
