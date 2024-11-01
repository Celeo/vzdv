use serde::Serialize;
use sqlx::{
    prelude::FromRow,
    types::chrono::{DateTime, Utc},
};

// Note: SQLite doesn't support u64.

#[derive(Debug, FromRow, Serialize, Clone, Default)]
pub struct Controller {
    pub id: u32,
    pub cid: u32,
    pub first_name: String,
    pub last_name: String,
    pub operating_initials: Option<String>,
    pub rating: i8,
    pub status: String,
    pub discord_id: Option<String>,
    pub home_facility: String,
    pub is_on_roster: bool,
    pub roles: String,
    pub join_date: Option<DateTime<Utc>>,
    pub loa_until: Option<DateTime<Utc>>,
}

#[derive(Debug, FromRow, Serialize, Clone)]
pub struct Certification {
    pub id: u32,
    pub cid: u32,
    pub name: String,
    /// "Training", "Solo", "Certified"
    pub value: String,
    pub changed_on: DateTime<Utc>,
    pub set_by: u32,
}

/// Requires joining the `controller` column for the name.
#[derive(Debug, FromRow, Serialize, Clone)]
pub struct Activity {
    pub id: u32,
    pub cid: u32,
    pub first_name: String,
    pub last_name: String,
    pub month: String,
    pub minutes: u32,
}

#[derive(Debug, FromRow, Serialize)]
pub struct Feedback {
    pub id: u32,
    pub controller: u32,
    pub position: String,
    pub rating: String,
    pub comments: String,
    pub created_date: DateTime<Utc>,
    pub submitter_cid: u32,
    pub reviewed_by_cid: u32,
    pub reviewer_action: String,
    pub posted_to_discord: bool,
}

#[derive(Debug, FromRow, Serialize)]
pub struct FeedbackForReview {
    pub id: u32,
    pub first_name: String,
    pub last_name: String,
    pub position: String,
    pub rating: String,
    pub comments: String,
    pub created_date: DateTime<Utc>,
    pub submitter_cid: u32,
    pub reviewer_action: String,
}

#[derive(Debug, FromRow, Serialize, Default)]
pub struct Resource {
    pub id: u32,
    pub category: String,
    pub name: String,
    pub file_name: Option<String>,
    pub link: Option<String>,
    pub updated: DateTime<Utc>,
}

#[derive(Debug, FromRow, Serialize)]
pub struct VisitorRequest {
    pub id: u32,
    pub cid: u32,
    pub first_name: String,
    pub last_name: String,
    pub home_facility: String,
    pub rating: u8,
    pub date: DateTime<Utc>,
}

#[derive(Debug, FromRow, Serialize)]
pub struct Event {
    pub id: u32,
    pub published: bool,
    pub name: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub description: Option<String>,
    pub image_url: Option<String>,
}

#[derive(Debug, FromRow, Serialize)]
pub struct EventPosition {
    pub id: u32,
    pub event_id: u32,
    pub name: String,
    pub category: String,
    pub cid: Option<u32>,
}

#[derive(Debug, FromRow, Serialize)]
pub struct EventRegistration {
    pub id: u32,
    pub event_id: u32,
    pub cid: u32,
    pub choice_1: u32,
    pub choice_2: u32,
    pub choice_3: u32,
    pub notes: Option<String>,
}

#[derive(Debug, FromRow, Serialize)]
pub struct StaffNote {
    pub id: u32,
    pub cid: u32,
    pub by: u32,
    pub date: DateTime<Utc>,
    pub comment: String,
}

#[derive(Debug, FromRow, Serialize)]
pub struct EmailTemplate {
    pub id: u32,
    pub name: String,
    pub subject: String,
    pub body: String,
}

/// Statements to create tables. Only ran when the DB file does not exist,
/// so no migration or "IF NOT EXISTS" conditions need to be added.
pub const CREATE_TABLES: &str = r#"
CREATE TABLE controller (
    id INTEGER PRIMARY KEY NOT NULL,
    cid INTEGER NOT NULL UNIQUE,
    first_name TEXT NOT NULL,
    last_name TEXT NOT NULL,
    email TEXT,
    operating_initials TEXT,
    rating INTEGER,
    status TEXT,
    discord_id TEXT,
    home_facility TEXT,
    is_on_roster INTEGER,
    roles TEXT,
    join_date TEXT,
    loa_until TEXT
) STRICT;

CREATE TABLE certification (
    id INTEGER PRIMARY KEY NOT NULL,
    cid INTEGER NOT NULL,
    name TEXT NOT NULL,
    value TEXT NOT NULL,
    changed_on TEXT NOT NULL,
    set_by INTEGER NOT NULL
) STRICT;

CREATE TABLE feedback (
    id INTEGER PRIMARY KEY NOT NULL,
    controller INTEGER NOT NULL,
    position TEXT NOT NULL,
    rating TEXT NOT NULL,
    comments TEXT,
    created_date TEXT NOT NULL,
    submitter_cid INTEGER NOT NULL,
    reviewed_by_cid INTEGER,
    reviewer_action TEXT NOT NULL DEFAULT 'pending',
    posted_to_discord INTEGER NOT NULL DEFAULT FALSE
) STRICT;

CREATE TABLE activity (
    id INTEGER PRIMARY KEY NOT NULL,
    cid INTEGER NOT NULL,
    month TEXT NOT NULL,
    minutes INTEGER NOT NULL,

    FOREIGN KEY (cid) REFERENCES controller(cid)
) STRICT;

CREATE TABLE resource (
    id INTEGER PRIMARY KEY NOT NULL,
    category TEXT NOT NULL,
    name TEXT NOT NULL,
    file_name TEXT,
    link TEXT,
    updated TEXT NOT NULL
) STRICT;

CREATE TABLE visitor_request (
    id INTEGER PRIMARY KEY NOT NULL,
    cid INTEGER NOT NULL,
    first_name TEXT NOT NULL,
    last_name TEXT NOT NULL,
    home_facility TEXT NOT NULL,
    rating INTEGER NOT NULL,
    date TEXT NOT NULL
) STRICT;

CREATE TABLE event (
    id INTEGER PRIMARY KEY NOT NULL,
    created_by INTEGER NOT NULL,
    published INTEGER NOT NULL DEFAULT FALSE,
    name TEXT NOT NULL,
    start TEXT NOT NULL,
    end TEXT NOT NULL,
    description TEXT,
    image_url TEXT,

    FOREIGN KEY (created_by) REFERENCES controller(cid)
) STRICT;

CREATE TABLE event_position (
    id INTEGER PRIMARY KEY NOT NULL,
    event_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    category TEXT NOT NULL,
    cid INTEGER,

    FOREIGN KEY (event_id) REFERENCES event(id),
    FOREIGN KEY (cid) REFERENCES controller(cid)
) STRICT;

CREATE TABLE event_registration (
    id INTEGER PRIMARY KEY NOT NULL,
    event_id INTEGER NOT NULL,
    cid INTEGER NOT NULL,
    choice_1 INTEGER,
    choice_2 INTEGER,
    choice_3 INTEGER,
    notes TEXT,

    UNIQUE(event_id, cid),
    FOREIGN KEY (event_id) REFERENCES event(id),
    FOREIGN KEY (cid) REFERENCES controller(cid),
    FOREIGN KEY (choice_1) REFERENCES event_position(id),
    FOREIGN KEY (choice_2) REFERENCES event_position(id),
    FOREIGN KEY (choice_3) REFERENCES event_position(id)
) STRICT;

CREATE TABLE staff_note (
    id INTEGER PRIMARY KEY NOT NULL,
    cid INTEGER NOT NULL,
    by INTEGER NOT NULL,
    date TEXT NOT NULL,
    comment TEXT NOT NULL,

    FOREIGN KEY (cid) REFERENCES controller(cid),
    FOREIGN KEY (by) REFERENCES controller(cid)
) STRICT;

CREATE table email_template (
    id INTEGER PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    subject TEXT NOT NULL,
    body TEXT NOT NULL
) STRICT;
"#;

pub const UPSERT_USER_LOGIN: &str = "
INSERT INTO controller
    (id, cid, first_name, last_name, email, rating, is_on_roster)
VALUES
    (NULL, $1, $2, $3, $4, $5, FALSE)
ON CONFLICT(cid) DO UPDATE SET
    first_name=excluded.first_name,
    last_name=excluded.last_name,
    email=excluded.email,
    rating=excluded.rating
WHERE
    cid=excluded.cid
";
pub const UPSERT_USER_TASK: &str = "
INSERT INTO controller
    (id, cid, first_name, last_name, email, rating, home_facility, is_on_roster, join_date, roles)
VALUES
    (NULL, $1, $2, $3, $4, $5, $6, $7, $8, $9)
ON CONFLICT(cid) DO UPDATE SET
    first_name=excluded.first_name,
    last_name=excluded.last_name,
    email=excluded.email,
    rating=excluded.rating,
    home_facility=excluded.home_facility,
    is_on_roster=excluded.is_on_roster,
    join_date=excluded.join_date,
    roles=excluded.roles
WHERE
    cid=excluded.cid
";
pub const INSERT_USER_SIMPLE: &str = "INSERT INTO controller
    (id, cid, first_name, last_name, rating, home_facility, is_on_roster)
VALUES
    (NULL, $1, $2, $3, $4, $5, FALSE)";

pub const GET_ALL_CONTROLLERS: &str = "SELECT * FROM controller";
pub const GET_ALL_CONTROLLERS_ON_ROSTER: &str = "SELECT * FROM controller WHERE is_on_roster=TRUE";
pub const GET_ALL_CONTROLLERS_OFF_ROSTER: &str =
    "SELECT * FROM controller WHERE is_on_roster=FALSE";
pub const GET_ALL_CONTROLLER_CIDS: &str = "SELECT cid FROM controller";
pub const GET_ALL_ROSTER_CONTROLLER_CIDS: &str =
    "SELECT cid FROM controller WHERE is_on_roster=TRUE";
pub const UPDATE_REMOVED_FROM_ROSTER: &str =
    "UPDATE controller SET is_on_roster=0, home_facility='', join_date=NULL, operating_initials=NULL WHERE cid=$1";
pub const UPDATE_CONTROLLER_OIS: &str = "UPDATE controller SET operating_initials=$2 WHERE cid=$1";
pub const GET_ALL_OIS: &str = "SELECT operating_initials FROM controller";
pub const GET_CONTROLLER_BY_CID: &str = "SELECT * FROM controller WHERE cid=$1";
pub const GET_CONTROLLER_CIDS_AND_NAMES: &str = "SELECT cid, first_name, last_name from controller";
pub const GET_ATM_AND_DATM: &str = "SELECT * FROM controller WHERE roles LIKE '%ATM%'";
pub const GET_CONTROLLER_BY_DISCORD_ID: &str = "SELECT * FROM controller WHERE discord_id=$1";
pub const SET_CONTROLLER_DISCORD_ID: &str = "UPDATE controller SET discord_id=$2 WHERE cid=$1";
pub const UNSET_CONTROLLER_DISCORD_ID: &str = "UPDATE controller SET discord_id=NULL WHERE cid=$1";
pub const SET_CONTROLLER_ROLES: &str = "UPDATE controller SET roles=$2 WHERE cid=$1";

pub const GET_ALL_CERTIFICATIONS: &str = "SELECT * FROM certification";
pub const GET_ALL_CERTIFICATIONS_FOR: &str = "SELECT * FROM certification WHERE cid=$1";
pub const CREATE_CERTIFICATION: &str =
    "INSERT INTO certification VALUES (NULL, $1, $2, $3, $4, $5);";
pub const UPDATE_CERTIFICATION: &str =
    "UPDATE certification SET value=$2, changed_on=$3, set_by=$4 WHERE id=$1";

pub const GET_ALL_ACTIVITY: &str =
    "SELECT * FROM activity LEFT JOIN controller ON activity.cid = controller.cid";
pub const GET_ACTIVITY_IN_MONTH: &str =
    "SELECT activity.*, controller.first_name, controller.last_name FROM activity LEFT JOIN controller ON activity.cid = controller.cid WHERE month=$1 ORDER BY minutes DESC";
pub const DELETE_ACTIVITY_FOR_CID: &str = "DELETE FROM activity WHERE cid=$1";
pub const INSERT_INTO_ACTIVITY: &str = "
INSERT INTO activity
    (id, cid, month, minutes)
VALUES
    (NULL, $1, $2, $3)
";
pub const UPDATE_ACTIVITY: &str = "UPDATE activity SET minutes=$3 WHERE cid=$1 AND month=$2";

pub const INSERT_FEEDBACK: &str = "
INSERT INTO feedback
    (id, controller, position, rating, comments, created_date, submitter_cid)
VALUES
    (NULL, $1, $2, $3, $4, $5, $6)
";
pub const GET_ALL_PENDING_FEEDBACK: &str =
    "SELECT * FROM feedback WHERE reviewed_by_cid IS NULL OR reviewer_action='archive'";
pub const GET_PENDING_FEEDBACK_FOR_REVIEW: &str =
    "SELECT feedback.*, controller.first_name, controller.last_name FROM feedback LEFT JOIN controller ON feedback.controller = controller.cid";
pub const GET_FEEDBACK_BY_ID: &str = "SELECT * FROM feedback WHERE id=$1";
pub const UPDATE_FEEDBACK_TAKE_ACTION: &str =
    "UPDATE feedback SET reviewed_by_cid=$1, reviewer_action=$2, posted_to_discord=$3 WHERE id=$4";
pub const DELETE_FROM_FEEDBACK: &str = "DELETE FROM feedback WHERE id=$1";
pub const GET_ALL_FEEDBACK_FOR: &str = "SELECT * FROM feedback WHERE controller=$1";
pub const GET_APPROVED_FEEDBACK_FOR: &str =
    "SELECT * FROM feedback WHERE controller=$1 AND (reviewer_action='approve' OR reviewer_action='post')";
pub const UPDATE_FEEDBACK_COMMENTS: &str = "UPDATE feedback SET comments=$2 WHERE id=$1";

pub const GET_ALL_RESOURCES: &str = "SELECT * FROM resource";
pub const GET_RESOURCE_BY_ID: &str = "SELECT * FROM resource WHERE id=$1";
pub const DELETE_RESOURCE_BY_ID: &str = "DELETE FROM resource WHERE id=$1";
pub const CREATE_NEW_RESOURCE: &str = "INSERT INTO resource VALUES (NULL, $1, $2, $3, $4, $5)";

pub const GET_VISITOR_REQUEST_BY_ID: &str = "SELECT * FROM visitor_request WHERE id=$1";
pub const GET_ALL_VISITOR_REQUESTS: &str = "SELECT * FROM visitor_request";
pub const GET_PENDING_VISITOR_REQ_FOR: &str = "SELECT * FROM visitor_request WHERE cid=$1";
pub const INSERT_INTO_VISITOR_REQ: &str =
    "INSERT INTO visitor_request VALUES (NULL, $1, $2, $3, $4, $5, $6);";
pub const DELETE_VISITOR_REQUEST: &str = "DELETE FROM visitor_request WHERE id=$1";

pub const GET_UPCOMING_EVENTS: &str =
    "SELECT * FROM event WHERE end > $1 AND published = TRUE ORDER BY start ASC";
pub const GET_ALL_UPCOMING_EVENTS: &str = "SELECT * FROM event WHERE end > $1 ORDER BY start ASC";
pub const GET_EVENT: &str = "SELECT * FROM event WHERE id=$1";
pub const DELETE_EVENT: &str = "DELETE FROM event WHERE id=$1";
pub const CREATE_EVENT: &str = "INSERT INTO event VALUES (NULL, $1, FALSE, $2, $3, $4, $5, $6);";
pub const UPDATE_EVENT: &str = "UPDATE event SET name=$2, published=$3, start=$4, end=$5, description=$6, image_url=$7 where id=$1";

pub const GET_EVENT_REGISTRATION_FOR: &str =
    "SELECT * FROM event_registration WHERE event_id=$1 AND cid=$2";
pub const GET_EVENT_REGISTRATIONS: &str = "SELECT * FROM event_registration WHERE event_id=$1";
pub const DELETE_EVENT_REGISTRATION: &str = "DELETE FROM event_registration WHERE id=$1";
pub const UPSERT_EVENT_REGISTRATION: &str = "
INSERT INTO event_registration
    (event_id, cid, choice_1, choice_2, choice_3, notes)
VALUES
    ($1, $2, $3, $4, $5, $6)
ON CONFLICT DO UPDATE SET
    choice_1=$3,
    choice_2=$4,
    choice_3=$5,
    notes=$6";
pub const CLEAR_REGISTRATIONS_FOR_POSITION_1: &str =
    "UPDATE event_registration SET choice_1=NULL WHERE choice_1=$1";
pub const CLEAR_REGISTRATIONS_FOR_POSITION_2: &str =
    "UPDATE event_registration SET choice_2=NULL WHERE choice_2=$1";
pub const CLEAR_REGISTRATIONS_FOR_POSITION_3: &str =
    "UPDATE event_registration SET choice_3=NULL WHERE choice_3=$1";
pub const DELETE_REGISTRATIONS_NOW_EMPTY: &str =
    "DELETE from event_registration WHERE event_id=$1 AND choice_1 is NULL AND choice_2 is NULL AND choice_3 is NULL";

pub const GET_EVENT_POSITIONS: &str = "SELECT * FROM event_position WHERE event_id=$1";
pub const INSERT_EVENT_POSITION: &str =
    "INSERT INTO event_position VALUES (NULL, $1, $2, $3, NULL);";
pub const DELETE_EVENT_POSITION: &str = "DELETE FROM event_position WHERE id=$1";
pub const UPDATE_EVENT_POSITION_CONTROLLER: &str = "UPDATE event_position SET cid=$2 WHERE id=$1";
pub const CLEAR_CID_FROM_EVENT_POSITIONS: &str =
    "UPDATE event_position SET cid=NULL WHERE event_id=$1 AND cid=$2";

pub const GET_STAFF_NOTES_FOR: &str = "SELECT * FROM staff_note WHERE cid=$1";
pub const GET_STAFF_NOTE: &str = "SELECT * FROM staff_note WHERE id=$1";
pub const DELETE_STAFF_NOTE: &str = "DELETE FROM staff_note WHERE id=$1";
pub const CREATE_STAFF_NOTE: &str = "INSERT INTO staff_note VALUES (NULL, $1, $2, $3, $4);";

pub const GET_EMAIL_TEMPLATE: &str = "SELECT * FROM email_template WHERE name=$1";
pub const UPDATE_EMAIL_TEMPLATE: &str =
    "UPDATE email_template SET subject=$2, body=$3 WHERE name=$1";
