use anyhow::Result;
use chrono::Utc;
use log::info;
use sqlx::{Pool, Sqlite};
use vzdv::sql::{self, Certification, SoloCert};

/// Check the DB for freshly expired solo certs.
pub async fn check_expired(db: &Pool<Sqlite>) -> Result<()> {
    let solo_certs: Vec<SoloCert> = sqlx::query_as(sql::GET_ALL_SOLO_CERTS)
        .fetch_all(db)
        .await?;
    let now = Utc::now();

    for cert in solo_certs {
        if cert.expiration_date < now {
            info!(
                "Solo cert {} for {} on {} by {} is expired; deleting",
                cert.id, cert.cid, cert.position, cert.issued_by
            );
            // delete the solo_cert record
            sqlx::query(sql::DELETE_SOLO_CERT)
                .bind(cert.id)
                .execute(db)
                .await?;
            // get the controller's certifications
            let controller_certs: Vec<Certification> =
                sqlx::query_as(sql::GET_ALL_CERTIFICATIONS_FOR)
                    .bind(cert.cid)
                    .fetch_all(db)
                    .await?;
            for c_cert in controller_certs {
                // if the certification was set to "solo" by training staff, set it back to "training"
                if c_cert.value == "solo" {
                    info!(
                        "Setting certification {} for {} back to 'training' from 'solo'",
                        c_cert.id, c_cert.cid
                    );
                    sqlx::query(sql::UPDATE_CERTIFICATION)
                        .bind(c_cert.id)
                        .bind("training")
                        .bind(c_cert.changed_on)
                        .bind(c_cert.set_by)
                        .execute(db)
                        .await?;
                }
            }
        }
    }

    Ok(())
}
