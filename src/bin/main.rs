//! New vZVD website.

#![deny(clippy::all)]

use anyhow::Result;
use axum::{middleware as axum_middleware, response::Redirect, Router};
use clap::Parser;
use log::{debug, error, info};
use mini_moka::sync::Cache;
use minijinja::Environment;
use std::{
    env,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::signal;
use tower::ServiceBuilder;
use tower_http::timeout::TimeoutLayer;
use tower_sessions::SessionManagerLayer;
use tower_sessions_sqlx_store::SqliteStore;
use vzdv::{
    load_config, load_db,
    shared::{self, AppState},
};

/// vZDV website.
#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    /// Load the config from a specific file.
    ///
    /// [default: site_config.toml]
    #[arg(long)]
    config: Option<PathBuf>,

    /// Enable debug logging
    #[arg(short, long)]
    debug: bool,

    /// Host to run on
    #[arg(long, default_value = "0.0.0.0")]
    host: String,

    /// Port to run on
    #[arg(long, default_value_t = 3000)]
    port: u16,
}

/// Load all template files into the binary via the stdlib `include_str!`
/// macro and supply to the minijinja environment.
fn load_templates() -> Result<Environment<'static>> {
    let mut env = Environment::new();
    env.add_template("_layout", include_str!("../../templates/_layout.jinja"))?;
    Ok(env)
}

/// Create all the endpoints and insert middleware.
fn load_router(
    sessions_layer: SessionManagerLayer<SqliteStore>,
    env: &mut Environment,
) -> Router<Arc<AppState>> {
    Router::new()
        .merge(vzdv::endpoints::router(env))
        .merge(vzdv::endpoints::homepage::router(env))
        .merge(vzdv::endpoints::auth::router(env))
        .merge(vzdv::endpoints::airspace::router(env))
        .merge(vzdv::endpoints::facility::router(env))
        .merge(vzdv::endpoints::user::router(env))
        .layer(
            ServiceBuilder::new()
                .layer(TimeoutLayer::new(Duration::from_secs(30)))
                .layer(axum_middleware::from_fn(vzdv::middleware::logging))
                .layer(sessions_layer),
        )
        .fallback(|| async { Redirect::to("/404") })
}

// https://github.com/tokio-rs/axum/blob/main/examples/graceful-shutdown/src/main.rs
async fn shutdown_signal() {
    let ctrl_c = async {
        debug!("Got terminate signal");
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        debug!("Got terminate signal");
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

/// Entrypoint.
#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if cli.debug {
        env::set_var("RUST_LOG", "info,tracing::span=warn,vzdv=debug");
    } else if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "tracing::span=warn,info");
    }
    pretty_env_logger::init();
    debug!("Logging configured");

    debug!("Loading");
    let config_location = match cli.config {
        Some(path) => path,
        None => Path::new(shared::DEFAULT_CONFIG_FILE_NAME).to_owned(),
    };
    debug!(
        "> Loading from config file at: {}",
        config_location.display()
    );
    let config = match load_config(&config_location) {
        Ok(c) => c,
        Err(e) => {
            error!("Could not load config: {e}");
            std::process::exit(1);
        }
    };
    let db = match load_db(&config).await {
        Ok(db) => db,
        Err(e) => {
            error!("Could not load DB: {e}");
            std::process::exit(1);
        }
    };
    let sessions = SqliteStore::new(db.clone());
    if let Err(e) = sessions.migrate().await {
        error!("Could not create table for sessions: {e}");
        return;
    }
    let session_layer = SessionManagerLayer::new(sessions);
    let mut templates = match load_templates() {
        Ok(t) => t,
        Err(e) => {
            error!("Could not load the first templates: {e}");
            return;
        }
    };
    let cache = Cache::new(10);
    debug!("Loaded");

    debug!("Setting up app");
    let router = load_router(session_layer, &mut templates);
    let app_state = Arc::new(AppState {
        config,
        db: db.clone(),
        templates,
        cache,
    });
    let app = router.with_state(app_state);
    debug!("Set up");

    let host_and_port = format!("{}:{}", cli.host, cli.port);
    info!("Listening on http://{host_and_port}/");
    let listener = tokio::net::TcpListener::bind(&host_and_port)
        .await
        .expect("Could not bind the HTTP listener");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("Could not serve the app");
    db.close().await;
}
