use crate::shared::AppError;
use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use minijinja::{context, Environment};
use sqlx::{Pool, Sqlite};
use vzdv::config::Config;
use vzdv::sql::{self, Controller, EmailTemplate};

/// Email template names.
pub mod templates {
    pub const VISITOR_ACCEPTED: &str = "visitor_accepted";
    pub const VISITOR_DENIED: &str = "visitor_denied";
    pub const VISITOR_REMOVED: &str = "visitor_removed";
}

/// Email templates by name.
pub struct Templates {
    pub visitor_accepted: EmailTemplate,
    pub visitor_denied: EmailTemplate,
    pub visitor_removed: EmailTemplate,
}

/// Send an SMTP email to the recipient.
pub async fn send_mail(
    config: &Config,
    db: &Pool<Sqlite>,
    recipient_name: &str,
    recipient_address: &str,
    template_name: &str,
) -> Result<(), AppError> {
    let template = query_template(db, template_name).await?;

    // ATM and DATM names for signing
    let atm_datm: Vec<Controller> = sqlx::query_as(sql::GET_ATM_AND_DATM).fetch_all(db).await?;
    let atm = atm_datm
        .iter()
        .find(|controller| controller.roles.contains("ATM") && !controller.roles.contains("DATM"))
        .map(|controller| format!("{} {}, ATM", controller.first_name, controller.last_name))
        .unwrap_or_default();
    let datm = atm_datm
        .iter()
        .find(|controller| controller.roles.contains("DATM"))
        .map(|controller| format!("{} {}, DATM", controller.first_name, controller.last_name))
        .unwrap_or_default();

    // template load and render
    let mut env = Environment::new();
    env.add_template("body", &template.body)?;
    let body = env
        .get_template("body")?
        .render(context! { recipient_name, atm, datm })?;

    // construct and send email
    let email = Message::builder()
        .from(config.email.from.parse().unwrap())
        .reply_to(config.email.reply_to.parse().unwrap())
        .to(recipient_address.parse().unwrap())
        .subject(template.subject.to_owned())
        .header(ContentType::TEXT_PLAIN)
        .body(body)
        .unwrap();
    let creds = Credentials::new(
        config.email.user.to_owned(),
        config.email.password.to_owned(),
    );
    let mailer = SmtpTransport::relay(&config.email.host)
        .unwrap()
        .credentials(creds)
        .build();
    mailer.send(&email)?;
    Ok(())
}

/// Get a single template by name.
///
/// Returns an error if the template does not exist.
pub async fn query_template(db: &Pool<Sqlite>, template: &str) -> Result<EmailTemplate, AppError> {
    let template = sqlx::query_as(sql::GET_EMAIL_TEMPLATE)
        .bind(template)
        .fetch_one(db)
        .await?;
    Ok(template)
}

/// Load email templates from the database.
pub async fn query_templates(db: &Pool<Sqlite>) -> Result<Templates, AppError> {
    let visitor_accepted: EmailTemplate = sqlx::query_as(sql::GET_EMAIL_TEMPLATE)
        .bind(templates::VISITOR_ACCEPTED)
        .fetch_one(db)
        .await?;
    let visitor_denied: EmailTemplate = sqlx::query_as(sql::GET_EMAIL_TEMPLATE)
        .bind(templates::VISITOR_DENIED)
        .fetch_one(db)
        .await?;
    let visitor_removed: EmailTemplate = sqlx::query_as(sql::GET_EMAIL_TEMPLATE)
        .bind(templates::VISITOR_REMOVED)
        .fetch_one(db)
        .await?;
    Ok(Templates {
        visitor_accepted,
        visitor_denied,
        visitor_removed,
    })
}
