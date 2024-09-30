use anyhow::Result;
use log::{debug, error, info};
use sqlx::{Pool, Sqlite};
use std::{sync::Arc, time::Duration};
use tokio::time::sleep;
use twilight_http::Client;
use twilight_model::{
    guild::Member,
    id::{marker::GuildMarker, Id},
};
use vzdv::{
    config::Config,
    sql::{self, Controller},
    ControllerRating,
};

/// Set the guild member's nickname if needed.
async fn set_nickname(
    guild_id: Id<GuildMarker>,
    member: &Member,
    controller: &Controller,
    http: &Arc<Client>,
) -> Result<()> {
    let mut name = format!(
        "{} {}.",
        controller.first_name,
        controller.last_name.chars().next().unwrap()
    );
    if let Some(ois) = &controller.operating_initials {
        name.push_str(" - ");
        name.push_str(ois);
    }

    if controller.roles.contains("ATM") {
        name.push_str(" | ATM");
    } else if controller.roles.contains("DATM") {
        name.push_str(" | DATM");
    } else if controller.roles.contains("TA") {
        name.push_str(" | TA");
    } else if controller.roles.contains("EC") {
        name.push_str(" | EC");
    } else if controller.roles.contains("FE") {
        name.push_str(" | FE");
    } else if controller.roles.contains("WM") {
        name.push_str(" | WM");
    } else if controller.roles.contains("AEC") {
        name.push_str(" | AEC");
    } else if controller.roles.contains("AFE") {
        name.push_str(" | AFE");
    } else if controller.roles.contains("AWM") {
        name.push_str(" | AWM");
    } else if controller.roles.contains("MTR") {
        name.push_str(" | MTR");
    }

    if let Some(existing) = &member.nick {
        if existing != &name {
            info!("Updating nick of {} to {name}", member.user.id);
            http.update_guild_member(guild_id, member.user.id)
                .nick(Some(&name))?
                .await?;
        }
    } else {
        info!("Setting nick of {} to {name}", member.user.id);
        http.update_guild_member(guild_id, member.user.id)
            .nick(Some(&name))?
            .await?;
    }

    Ok(())
}

/// Resolve the guild member's roles, adding and removing as necessary.
async fn resolve_roles(
    guild_id: Id<GuildMarker>,
    member: &Member,
    roles: &[(u64, bool)],
    http: &Arc<Client>,
) -> Result<()> {
    // TODO

    let existing: Vec<_> = member.roles.iter().map(|r| r.get()).collect();
    for &(id, should_have) in roles {
        if should_have && !existing.contains(&id) {
            debug!(
                "Adding {id} to {} ({})",
                member.nick.as_ref().unwrap_or(&member.user.name),
                member.user.id.get()
            );
            // http.add_guild_member_role(guild_id, member.user.id, Id::new(id))
            //     .await?;
        } else if !should_have && existing.contains(&id) {
            debug!(
                "Removing {id} from {} ({})",
                member.nick.as_ref().unwrap_or(&member.user.name),
                member.user.id.get()
            );
            // http.remove_guild_member_role(guild_id, member.user.id, Id::new(id))
            //     .await?;
        }
    }
    Ok(())
}

/// Determine which roles the guild member should have.
async fn get_correct_roles(
    config: &Arc<Config>,
    member: &Member,
    controller: &Controller,
) -> Result<Vec<(u64, bool)>> {
    debug!("Processing user {}", member.user.id);
    let mut to_resolve = Vec::with_capacity(15);

    // membership
    to_resolve.push((
        config.discord.roles.home_controller,
        controller.home_facility == "ZDV",
    ));
    to_resolve.push((
        config.discord.roles.visiting_controller,
        controller.is_on_roster && controller.home_facility != "ZDV",
    ));
    to_resolve.push((config.discord.roles.guest, !controller.is_on_roster));

    // network rating
    to_resolve.push((
        config.discord.roles.administrator,
        controller.rating == ControllerRating::ADM.as_id(),
    ));
    to_resolve.push((
        config.discord.roles.supervisor,
        controller.rating == ControllerRating::SUP.as_id(),
    ));
    to_resolve.push((
        config.discord.roles.instructor_3,
        controller.rating == ControllerRating::I3.as_id(),
    ));
    to_resolve.push((
        config.discord.roles.instructor_1,
        controller.rating == ControllerRating::I1.as_id(),
    ));
    to_resolve.push((
        config.discord.roles.controller_3,
        controller.rating == ControllerRating::C3.as_id(),
    ));
    to_resolve.push((
        config.discord.roles.controller_1,
        controller.rating == ControllerRating::C1.as_id(),
    ));
    to_resolve.push((
        config.discord.roles.student_3,
        controller.rating == ControllerRating::S3.as_id(),
    ));
    to_resolve.push((
        config.discord.roles.student_2,
        controller.rating == ControllerRating::S2.as_id(),
    ));
    to_resolve.push((
        config.discord.roles.student_1,
        controller.rating == ControllerRating::S1.as_id(),
    ));
    to_resolve.push((
        config.discord.roles.observer,
        controller.rating == ControllerRating::OBS.as_id(),
    ));

    // staff
    if ["ATM", "DATM", "TA"]
        .iter()
        .any(|role| controller.roles.contains(role))
    {
        to_resolve.push((config.discord.roles.sr_staff, true));
        to_resolve.push((config.discord.roles.jr_staff, false));
    } else if ["EC", "FE", "WM"]
        .iter()
        .any(|role| controller.roles.contains(role))
    {
        to_resolve.push((config.discord.roles.sr_staff, false));
        to_resolve.push((config.discord.roles.jr_staff, true));
    } else {
        to_resolve.push((config.discord.roles.sr_staff, false));
        to_resolve.push((config.discord.roles.jr_staff, false));
    }
    // Note: probably will let "staff teams" be manually assigned, same with VATUSA/VATGOV

    Ok(to_resolve)
}

/// Single loop execution.
async fn tick(config: &Arc<Config>, db: &Pool<Sqlite>, http: &Arc<Client>) -> Result<()> {
    info!("Role tick");
    let guild_id = Id::new(config.discord.guild_id);
    let members = http
        .guild_members(guild_id)
        .limit(3)?
        .await?
        .model()
        .await?;
    for member in &members {
        if member.user.id.get() == config.discord.owner_id {
            debug!(
                "Skipping over guild owner {} ({})",
                member.nick.as_ref().unwrap_or(&member.user.name),
                member.user.id.get()
            );
            continue;
        }
        if member.user.bot {
            debug!(
                "Skipping over bot user {} ({})",
                member.nick.as_ref().unwrap_or(&member.user.name),
                member.user.id.get()
            );
            continue;
        }
        debug!("Processing user {}", member.user.id);
        let controller: Option<Controller> = sqlx::query_as(sql::GET_CONTROLLER_BY_DISCORD_ID)
            .bind(member.user.id.get().to_string())
            .fetch_optional(db)
            .await?;
        let controller = match controller {
            Some(c) => c,
            None => {
                // no linked controller; strip all roles
                info!(
                    "No linked controller record; stripping all roles from {} ({})",
                    member.nick.as_ref().unwrap_or(&member.user.name),
                    member.user.id.get()
                );
                for role in &member.roles {
                    debug!(
                        "Removing {role} from {} ({})",
                        member.nick.as_ref().unwrap_or(&member.user.name),
                        member.user.id.get()
                    );
                    if let Err(e) = http
                        .remove_guild_member_role(guild_id, member.user.id, *role)
                        .await
                    {
                        error!(
                            "Could not remove role {role} from {} ({}): {e}",
                            member.nick.as_ref().unwrap_or(&member.user.name),
                            member.user.id.get()
                        );
                    }
                }
                continue;
            }
        };

        // roles
        debug!(
            "Determining roles to resolve for {} ({})",
            member.nick.as_ref().unwrap_or(&member.user.name),
            member.user.id.get()
        );

        // determine the roles the guild member should have and update accordingly
        match get_correct_roles(config, member, &controller).await {
            Ok(to_resolve) => {
                if let Err(e) = resolve_roles(guild_id, member, &to_resolve, http).await {
                    error!(
                        "Error resolving roles for {} ({}): {e}",
                        member.nick.as_ref().unwrap_or(&member.user.name),
                        member.user.id.get()
                    );
                }
            }
            Err(e) => {
                error!(
                    "Error determining roles for {} ({}): {e}",
                    member.nick.as_ref().unwrap_or(&member.user.name),
                    member.user.id.get()
                );
            }
        }

        // nickname
        if let Err(e) = set_nickname(guild_id, member, &controller, http).await {
            error!(
                "Error setting nickname of {} ({}): {e}",
                member.nick.as_ref().unwrap_or(&member.user.name),
                member.user.id.get()
            );
        };
    }

    Ok(())
}

// Processing loop.
pub async fn process(config: Arc<Config>, db: Pool<Sqlite>, http: Arc<Client>) {
    sleep(Duration::from_secs(30)).await;
    debug!("Starting roles processing");

    loop {
        if let Err(e) = tick(&config, &db, &http).await {
            error!("Error in roles processing tick: {e}");
        }
        sleep(Duration::from_secs(60 * 5)).await; // 5 minutes
    }
}
