//! Endpoints for viewing and registering for events.
//!
//! The CRUD of events themselves is under /admin routes.

use crate::{
    flashed_messages,
    shared::{
        is_user_member_of, js_timestamp_to_utc, reject_if_not_in, AppError, AppState, UserInfo,
        SESSION_USER_INFO_KEY,
    },
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Form, Router,
};
use axum_extra::extract::WithRejection;
use chrono::Utc;
use log::info;
use minijinja::{context, Environment};
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite};
use std::sync::Arc;
use tower_sessions::Session;
use vzdv::{
    sql::{self, Controller, Event, EventPosition, EventRegistration},
    ControllerRating, PermissionsGroup,
};

/// Get a list of upcoming events optionally with unpublished events.
async fn query_for_events(db: &Pool<Sqlite>, show_all: bool) -> sqlx::Result<Vec<Event>> {
    if show_all {
        sqlx::query_as(sql::GET_ALL_UPCOMING_EVENTS)
            .bind(Utc::now())
            .fetch_all(db)
            .await
    } else {
        sqlx::query_as(sql::GET_UPCOMING_EVENTS)
            .bind(Utc::now())
            .fetch_all(db)
            .await
    }
}

/// Render a snippet that lists published upcoming events.
///
/// No controls are rendered; instead each event links to the full
/// page for that single event.
async fn snippet_get_upcoming_events(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Html<String>, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    let show_all = is_user_member_of(&state, &user_info, PermissionsGroup::EventsTeam).await;
    let events = query_for_events(&state.db, show_all).await?;
    let template = state
        .templates
        .get_template("events/upcoming_events_snippet")?;
    let rendered = template.render(context! { user_info, events })?;
    Ok(Html(rendered))
}

/// Render a full page of upcoming events.
///
/// Basically what the homepage does, but without the rest of the homepage.
async fn get_upcoming_events(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Html<String>, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    let show_all = is_user_member_of(&state, &user_info, PermissionsGroup::EventsTeam).await;
    let events = query_for_events(&state.db, show_all).await?;
    let is_event_staff = is_user_member_of(&state, &user_info, PermissionsGroup::EventsTeam).await;
    let template = state.templates.get_template("events/upcoming_events")?;
    let rendered = template.render(context! { user_info, is_event_staff, events })?;
    Ok(Html(rendered))
}

#[derive(Debug, Deserialize)]
struct CreateEventForm {
    name: String,
    description: String,
    banner: String,
    start: String,
    end: String,
    timezone: String,
}

/// Submit the form to create a new event.
///
/// Event staff only.
async fn post_new_event_form(
    State(state): State<Arc<AppState>>,
    session: Session,
    WithRejection(Form(create_new_form), _): WithRejection<Form<CreateEventForm>, AppError>,
) -> Result<Redirect, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    let is_event_staff = is_user_member_of(&state, &user_info, PermissionsGroup::EventsTeam).await;
    if !is_event_staff {
        return Ok(Redirect::to("/"));
    }

    let cid = user_info.unwrap().cid;
    let start = js_timestamp_to_utc(&create_new_form.start, &create_new_form.timezone)?;
    let end = js_timestamp_to_utc(&create_new_form.end, &create_new_form.timezone)?;
    let result = sqlx::query(sql::CREATE_EVENT)
        .bind(cid)
        .bind(&create_new_form.name)
        .bind(start)
        .bind(end)
        .bind(create_new_form.description)
        .bind(create_new_form.banner)
        .execute(&state.db)
        .await?;
    info!(
        "{} created new event {}: \"{}\"",
        cid,
        result.last_insert_rowid(),
        &create_new_form.name
    );
    Ok(Redirect::to(&format!(
        "/events/{}",
        result.last_insert_rowid()
    )))
}

// NOTE: opportunity for some minor speed improvements here by not loading
// controller records twice for each controller assigned to an event.

/// Render the full page for a single event, including controls for signup.
async fn page_get_event(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(id): Path<u32>,
) -> Result<Response, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    let event: Option<Event> = sqlx::query_as(sql::GET_EVENT)
        .bind(id)
        .fetch_optional(&state.db)
        .await?;
    if let Some(event) = event {
        let not_staff_redirect =
            reject_if_not_in(&state, &user_info, PermissionsGroup::EventsTeam).await;
        if !event.published {
            // only event staff can see unpublished events
            if let Some(redirect) = not_staff_redirect {
                return Ok(redirect.into_response());
            }
        }
        let positions_raw: Vec<EventPosition> = sqlx::query_as(sql::GET_EVENT_POSITIONS)
            .bind(event.id)
            .fetch_all(&state.db)
            .await?;
        let positions = event_positions_extra(&positions_raw, &state.db).await?;
        let registrations = event_registrations_extra(event.id, &positions_raw, &state.db).await?;
        let all_controllers: Vec<Controller> = sqlx::query_as(sql::GET_ALL_CONTROLLERS_ON_ROSTER)
            .fetch_all(&state.db)
            .await?;
        let all_controllers: Vec<(u32, String)> = all_controllers
            .iter()
            .map(|controller| {
                (
                    controller.cid,
                    format!(
                        "{} {} ({})",
                        controller.first_name,
                        controller.last_name,
                        match controller.operating_initials.as_ref() {
                            Some(oi) => {
                                if oi.is_empty() {
                                    "??"
                                } else {
                                    oi
                                }
                            }
                            None => "??",
                        }
                    ),
                )
            })
            .collect();
        let template = state.templates.get_template("events/event")?;
        let self_register: Option<EventRegistration> = if let Some(user_info) = &user_info {
            sqlx::query_as(sql::GET_EVENT_REGISTRATION_FOR)
                .bind(id)
                .bind(user_info.cid)
                .fetch_optional(&state.db)
                .await?
        } else {
            None
        };

        let flashed_messages = flashed_messages::drain_flashed_messages(session).await?;
        let rendered = template.render(context! {
            user_info,
            event,
            positions,
            positions_raw,
            registrations,
            all_controllers,
            self_register,
            flashed_messages,
            is_event_staff => not_staff_redirect.is_none(),
            event_not_over =>  Utc::now() < event.end
        })?;
        Ok(Html(rendered).into_response())
    } else {
        flashed_messages::push_flashed_message(
            session,
            flashed_messages::MessageLevel::Error,
            "Event not found",
        )
        .await?;
        Ok(Redirect::to("/").into_response())
    }
}

#[derive(Serialize)]
struct EventPositionDisplay {
    id: u32,
    name: String,
    category: String,
    controller: String,
}

/// Supply event positions with the controller's name, if set.
async fn event_positions_extra(
    positions: &[EventPosition],
    db: &Pool<Sqlite>,
) -> Result<Vec<EventPositionDisplay>, AppError> {
    let mut ret = Vec::with_capacity(positions.len());
    for position in positions {
        if let Some(pos_cid) = position.cid {
            let controller: Option<Controller> = sqlx::query_as(sql::GET_CONTROLLER_BY_CID)
                .bind(pos_cid)
                .fetch_optional(db)
                .await?;
            if let Some(controller) = controller {
                ret.push(EventPositionDisplay {
                    id: position.id,
                    name: position.name.clone(),
                    category: position.category.clone(),
                    controller: format!(
                        "{} {} ({})",
                        controller.first_name,
                        controller.last_name,
                        match controller.operating_initials.as_ref() {
                            Some(oi) => oi,
                            None => "??",
                        }
                    ),
                });
                continue;
            }
        }
        ret.push(EventPositionDisplay {
            id: position.id,
            name: position.name.clone(),
            category: position.category.clone(),
            controller: "unassigned".to_string(),
        });
    }
    ret.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(ret)
}

#[derive(Serialize)]
struct EventRegistrationDisplay {
    controller: String,
    choice_1: String,
    choice_2: String,
    choice_3: String,
    notes: String,
}

/// Supply event registration data with controller and position names.
async fn event_registrations_extra(
    event_id: u32,
    positions: &[EventPosition],
    db: &Pool<Sqlite>,
) -> Result<Vec<EventRegistrationDisplay>, AppError> {
    let registrations: Vec<EventRegistration> = sqlx::query_as(sql::GET_EVENT_REGISTRATIONS)
        .bind(event_id)
        .fetch_all(db)
        .await?;
    let mut ret = Vec::with_capacity(registrations.len());

    for registration in &registrations {
        let c_1 = positions
            .iter()
            .find(|pos| pos.id == registration.choice_1)
            .map(|pos| pos.name.clone());
        let c_2 = positions
            .iter()
            .find(|pos| pos.id == registration.choice_2)
            .map(|pos| pos.name.clone());
        let c_3 = positions
            .iter()
            .find(|pos| pos.id == registration.choice_3)
            .map(|pos| pos.name.clone());
        let controller: Option<Controller> = sqlx::query_as(sql::GET_CONTROLLER_BY_CID)
            .bind(registration.cid)
            .fetch_optional(db)
            .await?;
        let controller = match controller {
            Some(c) => format!(
                "{} {} ({}) - {}",
                c.first_name,
                c.last_name,
                match c.operating_initials.as_ref() {
                    Some(oi) => oi,
                    None => "??",
                },
                ControllerRating::try_from(c.rating)
                    .map(|r| r.as_str())
                    .unwrap_or(""),
            ),
            None => "???".to_string(),
        };
        let notes = match registration.notes.as_ref() {
            Some(s) => s.clone(),
            None => String::new(),
        };
        ret.push(EventRegistrationDisplay {
            controller,
            choice_1: c_1.unwrap_or_default(),
            choice_2: c_2.unwrap_or_default(),
            choice_3: c_3.unwrap_or_default(),
            notes,
        });
    }

    Ok(ret)
}

#[derive(Deserialize)]
struct UpdateEventForm {
    name: String,
    description: String,
    published: Option<String>,
    banner: String,
    start: String,
    end: String,
    timezone: String,
}

/// Submit a form to update an event, and redirect back to the same page.
///
/// Event staff only.
async fn post_edit_event_form(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(id): Path<u32>,
    Form(details_form): Form<UpdateEventForm>,
) -> Result<Redirect, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    if let Some(redirect) = reject_if_not_in(&state, &user_info, PermissionsGroup::EventsTeam).await
    {
        return Ok(redirect);
    }

    let event: Option<Event> = sqlx::query_as(sql::GET_EVENT)
        .bind(id)
        .fetch_optional(&state.db)
        .await?;
    if event.is_some() {
        let start = js_timestamp_to_utc(&details_form.start, &details_form.timezone)?;
        let end = js_timestamp_to_utc(&details_form.end, &details_form.timezone)?;
        sqlx::query(sql::UPDATE_EVENT)
            .bind(id)
            .bind(details_form.name)
            .bind(details_form.published.is_some())
            .bind(start)
            .bind(end)
            .bind(details_form.description)
            .bind(details_form.banner)
            .execute(&state.db)
            .await?;
        info!("{} edited event {id}", user_info.unwrap().cid);
        Ok(Redirect::to(&format!("/events/{id}")))
    } else {
        Ok(Redirect::to("/"))
    }
}

/// API endpoint to delete an event.
///
/// Event staff only.
async fn api_delete_event(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(id): Path<u32>,
) -> Result<StatusCode, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    if !is_user_member_of(&state, &user_info, PermissionsGroup::EventsTeam).await {
        return Ok(StatusCode::FORBIDDEN);
    }
    let event: Option<Event> = sqlx::query_as(sql::GET_EVENT)
        .bind(id)
        .fetch_optional(&state.db)
        .await?;
    if event.is_some() {
        sqlx::query(sql::DELETE_EVENT)
            .bind(id)
            .execute(&state.db)
            .await?;
        info!("{} deleted event {id}", user_info.unwrap().cid);
        flashed_messages::push_flashed_message(
            session,
            flashed_messages::MessageLevel::Info,
            "Event deleted",
        )
        .await?;
        Ok(StatusCode::OK)
    } else {
        Ok(StatusCode::NOT_FOUND)
    }
}

#[derive(Deserialize)]
struct RegisterForm {
    choice_1: u32,
    choice_2: u32,
    choice_3: u32,
    notes: String,
}

/// Submit a form to register for an event or update a registration.
async fn post_register_for_event(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(id): Path<u32>,
    Form(register_data): Form<RegisterForm>,
) -> Result<Redirect, AppError> {
    let event: Option<Event> = sqlx::query_as(sql::GET_EVENT)
        .bind(id)
        .fetch_optional(&state.db)
        .await?;
    if event.is_none() {
        return Ok(Redirect::to("/events"));
    }
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    let cid = if let Some(user_info) = user_info {
        user_info.cid
    } else {
        return Ok(Redirect::to(&format!("/events/{id}")));
    };

    let c_1 = if register_data.choice_1 == 0u32 {
        None
    } else {
        Some(register_data.choice_1)
    };
    let c_2 = if register_data.choice_2 == 0u32 {
        None
    } else {
        Some(register_data.choice_2)
    };
    let c_3 = if register_data.choice_3 == 0u32 {
        None
    } else {
        Some(register_data.choice_3)
    };
    // upsert the registration
    sqlx::query(sql::UPSERT_EVENT_REGISTRATION)
        .bind(id)
        .bind(cid)
        .bind(c_1)
        .bind(c_2)
        .bind(c_3)
        .bind(&register_data.notes)
        .execute(&state.db)
        .await?;
    info!(
        "{cid} registered for event {id}: {} {} {}",
        c_1.unwrap_or_default(),
        c_2.unwrap_or_default(),
        c_3.unwrap_or_default()
    );

    Ok(Redirect::to(&format!("/events/{id}")))
}

/// Completely unregister a controller from an event.
async fn api_register_unregister(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(id): Path<u32>,
) -> Result<StatusCode, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    let cid = if let Some(user_info) = user_info {
        user_info.cid
    } else {
        return Ok(StatusCode::UNAUTHORIZED);
    };
    let existing_registration: Option<EventRegistration> =
        sqlx::query_as(sql::GET_EVENT_REGISTRATION_FOR)
            .bind(id)
            .bind(cid)
            .fetch_optional(&state.db)
            .await?;
    if let Some(existing) = existing_registration {
        sqlx::query(sql::DELETE_EVENT_REGISTRATION)
            .bind(existing.id)
            .execute(&state.db)
            .await?;
    }
    info!("{cid} removed their registration to event {id}");
    Ok(StatusCode::ACCEPTED)
}

#[derive(Deserialize)]
struct AddPositionForm {
    name: String,
    category: String,
}

/// Submit a form to add a new position to the event.
async fn post_add_position(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(id): Path<u32>,
    Form(new_position_data): Form<AddPositionForm>,
) -> Result<Redirect, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    if let Some(redirect) = reject_if_not_in(&state, &user_info, PermissionsGroup::EventsTeam).await
    {
        return Ok(redirect);
    }
    if new_position_data.name.is_empty() {
        flashed_messages::push_flashed_message(
            session,
            flashed_messages::MessageLevel::Error,
            "Must specify a value",
        )
        .await?;
        return Ok(Redirect::to(&format!("/events/{id}")));
    }

    let event: Option<Event> = sqlx::query_as(sql::GET_EVENT)
        .bind(id)
        .fetch_optional(&state.db)
        .await?;
    if event.is_some() {
        let name = new_position_data.name.to_uppercase();

        // don't allow position duplicates
        let existing: Vec<EventPosition> = sqlx::query_as(sql::GET_EVENT_POSITIONS)
            .bind(id)
            .fetch_all(&state.db)
            .await?;
        if !existing.iter().any(|position| {
            position.name == name && position.category == new_position_data.category
        }) {
            info!(
                "{} adding {}/{} to event {id}",
                user_info.unwrap().cid,
                &new_position_data.category,
                &name,
            );
            sqlx::query(sql::INSERT_EVENT_POSITION)
                .bind(id)
                .bind(new_position_data.name.to_uppercase())
                .bind(&new_position_data.category)
                .execute(&state.db)
                .await?;
        }
        Ok(Redirect::to(&format!("/events/{id}")))
    } else {
        Ok(Redirect::to("/"))
    }
}

/// Delete a position from the event.
async fn post_delete_position(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path((id, pos_id)): Path<(u32, u32)>,
) -> Result<Redirect, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    if let Some(redirect) = reject_if_not_in(&state, &user_info, PermissionsGroup::EventsTeam).await
    {
        return Ok(redirect);
    }

    let event: Option<Event> = sqlx::query_as(sql::GET_EVENT)
        .bind(id)
        .fetch_optional(&state.db)
        .await?;
    if event.is_some() {
        info!(
            "{} removed position {pos_id} from {id}",
            user_info.unwrap().cid,
        );
        sqlx::query(sql::DELETE_EVENT_POSITION)
            .bind(pos_id)
            .execute(&state.db)
            .await?;
        Ok(Redirect::to(&format!("/events/{id}")))
    } else {
        Ok(Redirect::to("/"))
    }
}

#[derive(Deserialize)]
struct SetPositionForm {
    position_id: u32,
    controller: u32,
}

/// Set a controller (or no-one) for a position.
async fn post_set_position(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(id): Path<u32>,
    Form(new_position_data): Form<SetPositionForm>,
) -> Result<Redirect, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    if let Some(redirect) = reject_if_not_in(&state, &user_info, PermissionsGroup::EventsTeam).await
    {
        return Ok(redirect);
    }

    let event: Option<Event> = sqlx::query_as(sql::GET_EVENT)
        .bind(id)
        .fetch_optional(&state.db)
        .await?;
    if event.is_some() {
        let cid = if new_position_data.controller != 0 {
            Some(new_position_data.controller)
        } else {
            None
        };
        sqlx::query(sql::UPDATE_EVENT_POSITION_CONTROLLER)
            .bind(new_position_data.position_id)
            .bind(cid)
            .execute(&state.db)
            .await?;
        info!(
            "{} updated event {id} position {} to cid {}",
            user_info.unwrap().cid,
            new_position_data.position_id,
            new_position_data.controller
        );
        Ok(Redirect::to(&format!("/events/{id}")))
    } else {
        Ok(Redirect::to("/"))
    }
}

/// This file's routes and templates.
pub fn router(template: &mut Environment) -> Router<Arc<AppState>> {
    template
        .add_template(
            "events/upcoming_events_snippet",
            include_str!("../../templates/events/upcoming_events_snippet.jinja"),
        )
        .unwrap();
    template
        .add_template(
            "events/upcoming_events",
            include_str!("../../templates/events/upcoming_events.jinja"),
        )
        .unwrap();
    template
        .add_template(
            "events/event",
            include_str!("../../templates/events/event.jinja"),
        )
        .unwrap();

    Router::new()
        .route("/events/upcoming", get(snippet_get_upcoming_events))
        .route(
            "/events",
            get(get_upcoming_events).post(post_new_event_form),
        )
        .route(
            "/events/:id",
            get(page_get_event)
                .delete(api_delete_event)
                .post(post_edit_event_form),
        )
        .route("/events/:id/register", post(post_register_for_event))
        .route("/events/:id/unregister", post(api_register_unregister))
        .route("/events/:id/add_position", post(post_add_position))
        .route(
            "/events/:id/delete_position/:pos_id",
            post(post_delete_position),
        )
        .route("/events/:id/set_position", post(post_set_position))
}
