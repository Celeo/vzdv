//! vZDV website background task runner.

#![deny(clippy::all)]
#![deny(unsafe_code)]

use clap::Parser;
use log::{debug, error, info};
use std::{path::PathBuf, time::Duration};
use tokio::time;
use vzdv::general_setup;

mod activity;
mod atis;
mod no_show_expiration;
mod roster;
mod solo_cert;
mod traffic_tracking;

/// vZDV task runner.
#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    /// Load the config from a specific file.
    ///
    /// [default: vzdv.toml]
    #[arg(long)]
    config: Option<PathBuf>,

    /// Enable debug logging
    #[arg(short, long)]
    debug: bool,
}

/// Entrypoint.
#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let (config, db) = general_setup(cli.debug, "vzdv_tasks", cli.config).await;

    info!("Starting tasks");

    let _roster_handle = {
        let db = db.clone();
        tokio::spawn(async move {
            debug!("Waiting 15 seconds before starting roster sync");
            time::sleep(Duration::from_secs(15)).await;
            for index in 0u64.. {
                /*
                 * Update the entire roster every 2 hours, but check for scheduled
                 * "quick updates" requested by users of the site every 5 minutes.
                 */
                if index.is_multiple_of(24) {
                    info!("Querying roster");
                    match roster::update_roster(&db).await {
                        Ok(_) => {
                            info!("Roster update successful");
                        }
                        Err(e) => {
                            error!("Error updating roster: {e}");
                        }
                    }
                } else if let Err(e) = roster::partial_update_roster(&db).await {
                    // don't log the success of this; individual CIDs will be logged in the function
                    error!("Error partial updating roster: {e}");
                }
                time::sleep(Duration::from_secs(60 * 5)).await;
            }
        })
    };

    let _activity_handle = {
        let config = config.clone();
        let db = db.clone();
        tokio::spawn(async move {
            debug!("Waiting 30 seconds before starting activity sync");
            time::sleep(Duration::from_secs(30)).await;
            for index in 0u64.. {
                /*
                 * Update everyone on a 6 hour schedule (15 minutes * 24 ticks = 6 hours).
                 * This update makes sure that everyone's data is accurate.
                 */
                if index.is_multiple_of(24) {
                    info!("Updating all activity");
                    match activity::true_up_all_controllers_activity(&config, &db).await {
                        Ok(_) => {
                            info!("Full activity update successful");
                        }
                        Err(e) => {
                            error!("Error updating full activity: {e}");
                        }
                    }
                } else {
                    /*
                     * Update online controllers every 15 minutes.
                     * This update might introduce small deltas in total time, but updates much
                     * faster for controllers that are actively controlling.
                     */
                    info!("Online controller activity check");
                    match activity::update_online_controller_activity(&config, &db).await {
                        Ok(_) => {
                            info!("Partial activity update successful");
                        }
                        Err(e) => {
                            error!("Error updating partial activity: {e}");
                        }
                    }
                }
                time::sleep(Duration::from_secs(60 * 15)).await;
            }
        })
    };

    let _solo_cert_handle = {
        let db: sqlx::Pool<sqlx::Sqlite> = db.clone();
        tokio::spawn(async move {
            debug!("Waiting 15 seconds before starting solo cert expiration check");
            time::sleep(Duration::from_secs(15)).await;
            loop {
                match solo_cert::check_expired(&db).await {
                    Ok(_) => {
                        debug!("Solo cert expiration checked");
                    }
                    Err(e) => {
                        error!("Error checking for solo cert expiration: {e}");
                    }
                }
                time::sleep(Duration::from_secs(60 * 30)).await;
            }
        })
    };

    let _no_show_expiration_handle = {
        let db: sqlx::Pool<sqlx::Sqlite> = db.clone();
        tokio::spawn(async move {
            debug!("Waiting 15 seconds before starting no-show expiration check");
            time::sleep(Duration::from_secs(15)).await;
            loop {
                match no_show_expiration::check_expired(&db).await {
                    Ok(_) => {
                        debug!("No-show expiration checked");
                    }
                    Err(e) => {
                        error!("Error checking for no-show expiration: {e}");
                    }
                }
                time::sleep(Duration::from_secs(60 * 60 * 12)).await;
            }
        })
    };

    let _online_data_handle = {
        let db: sqlx::Pool<sqlx::Sqlite> = db.clone();
        tokio::spawn(async move {
            debug!("Waiting 15 seconds before starting live data retrieval");
            time::sleep(Duration::from_secs(15)).await;
            loop {
                if let Err(e) = traffic_tracking::store_live_data(&db).await {
                    error!("Error getting VATSIM live data: {e}");
                }
                time::sleep(Duration::from_secs(15)).await;
            }
        })
    };

    let _atis_cleanup_handle = {
        let db = db.clone();
        tokio::spawn(async move {
            debug!("Waiting 15 seconds before starting ATIS cleanup");
            time::sleep(Duration::from_secs(15)).await;
            loop {
                if let Err(e) = atis::cleanup(&db).await {
                    error!("Error cleaning up ATIS data: {e}");
                }
                time::sleep(Duration::from_secs(60 * 5)).await;
            }
        })
    };

    _roster_handle.await.unwrap();
    _activity_handle.await.unwrap();
    _solo_cert_handle.await.unwrap();
    _no_show_expiration_handle.await.unwrap();
    _online_data_handle.await.unwrap();
    _atis_cleanup_handle.await.unwrap();

    db.close().await;
}
