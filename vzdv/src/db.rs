use crate::{config::Config, sql};
use anyhow::Result;
use log::warn;
use sqlx::{
    Executor, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous},
};
use std::path::Path;

/// Connect to the SQLite file at the destination, if it exists. If it does
/// not, a new file is created and statements to create tables are executed.
pub async fn load_db(config: &Config) -> Result<SqlitePool> {
    let options = SqliteConnectOptions::new()
        .filename(&config.database.file)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true);
    let pool = if !Path::new(&config.database.file).exists() {
        warn!("Creating new database file");
        let options = options.create_if_missing(true);
        let pool = SqlitePool::connect_with(options).await?;
        pool.execute(sql::CREATE_TABLES).await?;
        pool
    } else {
        SqlitePool::connect_with(options).await?
    };
    Ok(pool)
}
