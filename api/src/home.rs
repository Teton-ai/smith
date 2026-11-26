use crate::device::{SMITHD_SERVICE_NAME, Variable};
use crate::network::route::content_credentials;
use anyhow::Result;
use serde_json::Value;
use serde_json::json;
use smith::utils::schema;
use smith::utils::schema::SafeCommandTx::{UpdateNetwork, UpdateVariables};
use smith::utils::schema::{
    HomePost, NetworkType, SafeCommandRequest, SafeCommandResponse, SafeCommandRx, ServiceStatus,
};
use sqlx::{Acquire, PgConnection, PgPool};
use std::time::Duration;
use tokio::time::Instant;
use tracing::debug;
use tracing::error;

pub struct CommandsDB {
    id: i32,
    cmd: Value,
    continue_on_error: bool,
}

/// Postgres `json`/`jsonb` cannot store U+0000, even though it is valid UTF-8 and
/// valid JSON. Command output (e.g. `cat` on a binary file) can contain NUL bytes,
/// which would otherwise fail the insert and leave the command stuck as "executing".
/// Replace them with a visible sentinel so the byte isn't silently dropped.
fn sanitize_nul(value: &mut Value) {
    match value {
        Value::String(s) if s.contains('\0') => {
            *s = s.replace('\0', "\u{2400}"); // ␀ SYMBOL FOR NULL
        }
        Value::Array(a) => a.iter_mut().for_each(sanitize_nul),
        Value::Object(o) => o.values_mut().for_each(sanitize_nul),
        _ => {}
    }
}

fn map_key_mgmt(km: &str) -> Option<&'static str> {
    match km {
        "open" | "none" => Some("open"),
        "wpa-psk" => Some("wpa-psk"),
        "sae" => Some("sae"),
        "owe" => Some("owe"),
        "wpa-eap" => Some("wpa-eap"),
        other => {
            tracing::warn!(
                key_mgmt = other,
                "unknown key_mgmt in ReportNMProfiles; skipping security_type"
            );
            None
        }
    }
}

/// First key of the two key advisory locks on one command result; the network
/// ledger's single `bigint` locks are a separate key space.
const COMMAND_RESULT_LOCK: i32 = 2;
const SYSTEM_COMMAND_LOCK: i32 = 3;

/// Time a /home post may spend storing results. smithd gives up after 10 s and
/// sends the results again, so work after that only adds load.
const SAVE_BUDGET: Duration = Duration::from_secs(5);

/// Bounds each wait of a /home transaction, so an overload fails fast and
/// frees its connection.
pub(crate) const RESULT_TX_TIMEOUTS: &str =
    "SELECT set_config('lock_timeout', '1s', true), set_config('statement_timeout', '3s', true)";

/// Older smithd deadlocks above ~20 commands in one answer; the rest leave with later posts.
const COMMANDS_PER_ANSWER: i64 = 20;

/// The database is too busy now: the device should send its results again later.
pub fn is_overload(err: &anyhow::Error) -> bool {
    match err.downcast_ref::<sqlx::Error>() {
        Some(sqlx::Error::PoolTimedOut) => true,
        // lock_not_available (lock_timeout), query_canceled (statement_timeout)
        Some(sqlx::Error::Database(db)) => matches!(db.code().as_deref(), Some("55P03" | "57014")),
        _ => false,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Saved {
    All,
    /// Some results were not stored and must be sent again: another post of
    /// the device stores them now, or the post ran out of time.
    Partly,
}

/// Stores each result in its own short transaction on one connection, so no
/// lock lives longer than one result and no second connection is taken.
/// `Err` only when the database is overloaded; a result that fails for another
/// reason is logged and dropped, so it cannot block the device's queue.
pub async fn save_responses(
    device_id: i32,
    device_serial_number: &str,
    payload: HomePost,
    pool: &PgPool,
) -> Result<Saved> {
    if payload.responses.is_empty() {
        return Ok(Saved::All);
    }
    let deadline = Instant::now() + SAVE_BUDGET;
    let mut conn = pool.acquire().await?;
    let mut saved = Saved::All;
    for response in payload.responses {
        if Instant::now() >= deadline {
            return Ok(Saved::Partly);
        }
        let id = response.id;
        match save_response(&mut conn, device_id, device_serial_number, response).await {
            Ok(true) => {}
            Ok(false) => saved = Saved::Partly,
            Err(err) if is_overload(&err) => return Err(err),
            Err(err) => error!(
                device_id,
                command_id = id,
                "Failed to store result: {err:?}"
            ),
        }
    }
    Ok(saved)
}

/// `Ok(false)` when the result must be sent again: another post of the device
/// holds it now, or a network lock it needs is busy.
async fn save_response(
    conn: &mut PgConnection,
    device_id: i32,
    device_serial_number: &str,
    response: SafeCommandResponse,
) -> Result<bool> {
    let mut tx = conn.begin().await?;
    sqlx::query(RESULT_TX_TIMEOUTS).execute(&mut *tx).await?;

    // smithd sends a result again when its post failed, including one this api
    // stored while the device only saw a timeout. The try lock lets one post
    // store it and run its side effects; a post that overlaps does not wait.
    // The stored check runs after the lock, so its snapshot sees a commit made
    // before the lock was free. Scoped to the device so a result posted under
    // another device's command id cannot hide it. Synthetic results (negative
    // ids) are snapshots that overwrite, so they need neither.
    if response.id > 0 {
        let locked = sqlx::query_scalar!(
            r#"SELECT pg_try_advisory_xact_lock($1, $2) AS "locked!""#,
            COMMAND_RESULT_LOCK,
            response.id
        )
        .fetch_one(&mut *tx)
        .await?;
        // Ended here, not on drop: a dropped transaction rolls back only when
        // its connection is used again, and keeps the lock until then.
        if !locked {
            tx.rollback().await?;
            return Ok(false);
        }
        let stored = sqlx::query_scalar!(
            r#"SELECT EXISTS (
                   SELECT 1 FROM command_response WHERE command_id = $1 AND device_id = $2
               ) AS "stored!""#,
            response.id,
            device_id
        )
        .fetch_one(&mut *tx)
        .await?;
        if stored {
            tx.rollback().await?;
            return Ok(true);
        }
    }

    match response.command {
        SafeCommandRx::GetVariables => {
            let variables = sqlx::query_as!(
                Variable,
                "
                        SELECT id, device, name, value
                        FROM variable
                        WHERE device = $1
                        ORDER BY device, name
                        ",
                device_id
            )
            .fetch_all(&mut *tx)
            .await?;
            let update_variables = UpdateVariables {
                variables: variables
                    .into_iter()
                    .map(|variable| (variable.name, variable.value))
                    .collect(),
            };
            queue_latest(
                &mut tx,
                device_id,
                device_serial_number,
                SafeCommandRequest {
                    id: -1,
                    command: update_variables,
                    continue_on_error: false,
                },
            )
            .await?;
        }
        SafeCommandRx::GetNetwork => {
            let network = sqlx::query_as!(
                schema::Network,
                r#"
                        SELECT
                            n.id,
                            n.network_type::TEXT,
                            n.is_network_hidden,
                            n.ssid,
                            n.name,
                            n.description,
                            n.password
                        FROM network n
                        JOIN device d ON n.id = d.network_id
                        WHERE d.id = $1"#,
                &device_id
            )
            .fetch_optional(&mut *tx)
            .await?;

            if let Some(network) = network
                && network.network_type == NetworkType::Wifi
            {
                queue_latest(
                    &mut tx,
                    device_id,
                    device_serial_number,
                    SafeCommandRequest {
                        id: -4,
                        command: UpdateNetwork { network },
                        continue_on_error: false,
                    },
                )
                .await?;
            }
        }
        SafeCommandRx::ReportNMProfiles { ref profiles } if response.status == 0 => {
            let mut profiles_resolved: Vec<(i32, bool, String)> = Vec::new();

            // Advisory locks taken here are transaction-scoped, so they are all
            // held until this result's transaction commits. Taking them in nmcli
            // report order would deadlock two devices that see the same SSIDs in
            // a different order (A holds X wants Y, B holds Y wants X). Acquire
            // every lock up front in a deterministic order instead: the key is a
            // function of (ssid, hidden, psk), so sorting on that tuple gives
            // every writer the same sequence. The per-profile work below then
            // runs in report order, unaffected.
            //
            // Race-tested in e2e/tests/daemon_api.rs
            // (concurrent_identical_network_posts_converge_to_one_row).
            let mut lock_inputs: Vec<(&str, bool, Option<&str>)> = profiles
                .iter()
                .filter_map(|p| {
                    Some((
                        p.ssid.as_deref()?,
                        p.hidden.unwrap_or(false),
                        p.password.as_deref(),
                    ))
                })
                .collect();
            lock_inputs.sort_unstable();
            lock_inputs.dedup();

            // All devices on a site share these locks, so a post does not wait
            // on them. A busy lock skips a periodic snapshot (negative id): a
            // newer one follows and replaces it. A user command's result is
            // sent again.
            for (ssid, hidden, password) in &lock_inputs {
                let credentials = content_credentials(*password);
                let locked = sqlx::query_scalar!(
                    r#"SELECT pg_try_advisory_xact_lock(network_content_lock_key($1, $2, $3)) AS "locked!""#,
                    ssid,
                    hidden,
                    credentials,
                )
                .fetch_one(&mut *tx)
                .await?;
                if !locked {
                    tx.rollback().await?;
                    if response.id > 0 {
                        return Ok(false);
                    }
                    debug!(device_id, "Network lock busy; NM profile snapshot skipped");
                    return Ok(true);
                }
            }

            for profile in profiles {
                let ssid = match profile.ssid.as_deref() {
                    Some(s) => s,
                    None => continue,
                };

                let mapped_security_type: Option<&str> =
                    profile.key_mgmt.as_deref().and_then(map_key_mgmt);

                // Only for the writes below; mapped_security_type keeps its
                // relaxed-wildcard role in network_find_by_content. An
                // unrecognized key_mgmt (map_key_mgmt's `other => None`, a
                // permanent case, not backlog) can no longer write NULL once
                // network_security_type_wifi_check exists, so it falls back
                // here instead of failing the check-in. Same heuristic as
                // resolve_security_type / the Stage 1 backfill.
                let stored_security_type: &str =
                    mapped_security_type.unwrap_or(if profile.password.is_some() {
                        "wpa-psk"
                    } else {
                        "open"
                    });

                let mut creds_patch = serde_json::Map::new();
                if let Some(v) = &profile.pmf {
                    creds_patch.insert("pmf".into(), json!(v));
                }
                if let Some(v) = &profile.eap {
                    creds_patch.insert("eap".into(), json!(v));
                }
                if let Some(v) = &profile.phase2_auth {
                    creds_patch.insert("phase2_auth".into(), json!(v));
                }
                if let Some(v) = &profile.anonymous_identity {
                    creds_patch.insert("anonymous_identity".into(), json!(v));
                }
                let identity_val = profile
                    .eap_identity
                    .as_ref()
                    .map(|u| json!({"username": u}));
                let creds_patch = serde_json::Value::Object(creds_patch);

                // is_network_hidden is NOT NULL on the row; old smithd omitting `hidden`
                // resolves to the same default (false) the row would have been created
                // with, so a plain `=` match (as network_content_lock_key/find_by_content
                // require) is equivalent to the old relaxed-on-NULL match in practice.
                let hidden = profile.hidden.unwrap_or(false);
                let identity_credentials = content_credentials(profile.password.as_deref());

                // Shared with create_network (api/src/network/route.rs) so the match
                // cannot drift between writers; see network_find_by_content (arch.md).
                // ReportNMProfiles only ever describes wifi.
                // Only writer that knows an identity. Without it, two devices holding
                // different EAP credentials for one SSID match each other's row.
                let existing_id: Option<i32> = sqlx::query_scalar!(
                    r#"SELECT network_find_by_content($1, $2, $3, $4, 'wifi', $5)"#,
                    ssid,
                    hidden,
                    &identity_credentials,
                    mapped_security_type as Option<&str>,
                    identity_val.as_ref() as Option<&serde_json::Value>,
                )
                .fetch_one(&mut *tx)
                .await?;

                let network_id: i32 = match existing_id {
                    Some(id) => {
                        if creds_patch != json!({})
                            || identity_val.is_some()
                            || mapped_security_type.is_some()
                        {
                            // COALESCE never overwrites an already-typed row: the relaxed match
                            // (F4/F6) only lets a typed report reach a NULL row or an
                            // already-equal typed row, so filling NULL in place is always
                            // correcting unknown -> known, never clobbering a real value.
                            sqlx::query!(
                                r#"UPDATE network
                                       SET credentials = credentials || $2::jsonb,
                                           identity    = COALESCE($3::jsonb, identity),
                                           security_type = COALESCE(security_type, $4)
                                       WHERE id = $1"#,
                                id,
                                creds_patch,
                                identity_val as Option<serde_json::Value>,
                                stored_security_type,
                            )
                            .execute(&mut *tx)
                            .await?;
                        }
                        id
                    }
                    None => {
                        // The stored envelope starts from the same identity_credentials the
                        // lock and the match were computed from, never from security_type:
                        // deriving it from security_type dropped the psk whenever the type
                        // was unknown (old smithd, or an nmcli key_mgmt outside map_key_mgmt),
                        // which made the row unfindable by the key that had just created it,
                        // so every later report forked another duplicate. The Stage 2 metadata
                        // patch is merged on top in SQL.
                        sqlx::query_scalar!(
                            r#"INSERT INTO network
                                       (ssid, password, name, network_type, is_network_hidden,
                                        security_type, credentials, identity)
                                   VALUES ($1, $2, $1, 'wifi', $3, $4, $5::jsonb || $6::jsonb, $7)
                                   RETURNING id"#,
                            ssid,
                            profile.password,
                            hidden,
                            stored_security_type,
                            identity_credentials,
                            creds_patch,
                            identity_val as Option<serde_json::Value>,
                        )
                        .fetch_one(&mut *tx)
                        .await?
                    }
                };

                profiles_resolved.push((network_id, profile.is_active, profile.name.clone()));
            }

            // Guard against duplicate NM profile names (broken NM state where two
            // connections share the same connection.id). Keep the first occurrence so
            // the INSERT doesn't hit the (device_id, profile_name) PK constraint.
            let mut seen_names = std::collections::HashSet::new();
            profiles_resolved.retain(|(_, _, name)| seen_names.insert(name.clone()));

            sqlx::query!(
                "DELETE FROM device_configured_network WHERE device_id = $1",
                device_id
            )
            .execute(&mut *tx)
            .await?;

            if !profiles_resolved.is_empty() {
                let network_ids: Vec<i32> =
                    profiles_resolved.iter().map(|(id, _, _)| *id).collect();
                let is_active_flags: Vec<bool> =
                    profiles_resolved.iter().map(|(_, a, _)| *a).collect();
                let profile_names: Vec<String> = profiles_resolved
                    .iter()
                    .map(|(_, _, n)| n.clone())
                    .collect();
                sqlx::query!(
                        r#"INSERT INTO device_configured_network (device_id, network_id, is_active, profile_name)
                           SELECT $1, UNNEST($2::int[]), UNNEST($3::bool[]), UNNEST($4::text[])"#,
                        device_id,
                        &network_ids,
                        &is_active_flags,
                        &profile_names as &[String]
                    )
                    .execute(&mut *tx)
                    .await?;
            }

            let current_network_id = profiles_resolved
                .iter()
                .find(|(_, is_active, _)| *is_active)
                .map(|(id, _, _)| *id);

            sqlx::query!(
                "UPDATE device SET current_network_id = $2 WHERE id = $1",
                device_id,
                current_network_id
            )
            .execute(&mut *tx)
            .await?;
        }
        SafeCommandRx::ReportNMProfiles { .. } => {
            error!(
                device_id,
                "Partial NM profile snapshot (some detail lookups failed); preserving existing state"
            );
        }
        SafeCommandRx::WifiScan { ref networks } if response.status == 0 => {
            sqlx::query!(
                "DELETE FROM wifi_scan_result WHERE device_id = $1",
                device_id
            )
            .execute(&mut *tx)
            .await?;

            if !networks.is_empty() {
                let ssids: Vec<Option<String>> = networks.iter().map(|n| n.ssid.clone()).collect();
                let bssids: Vec<String> = networks.iter().map(|n| n.bssid.clone()).collect();
                let signals: Vec<Option<i32>> = networks.iter().map(|n| n.signal).collect();
                let rates: Vec<Option<i32>> = networks.iter().map(|n| n.rate).collect();
                let securities: Vec<Option<String>> =
                    networks.iter().map(|n| n.security.clone()).collect();
                let channels: Vec<Option<i32>> = networks.iter().map(|n| n.channel).collect();

                sqlx::query!(
                        r#"INSERT INTO wifi_scan_result (device_id, ssid, bssid, signal, rate, security, channel)
                           SELECT $1, UNNEST($2::text[]), UNNEST($3::text[]), UNNEST($4::int[]),
                                  UNNEST($5::int[]), UNNEST($6::text[]), UNNEST($7::int[])"#,
                        device_id,
                        &ssids as &[Option<String>],
                        &bssids as &[String],
                        &signals as &[Option<i32>],
                        &rates as &[Option<i32>],
                        &securities as &[Option<String>],
                        &channels as &[Option<i32>],
                    )
                    .execute(&mut *tx)
                    .await?;
            }
        }
        SafeCommandRx::WifiScan { .. } => {
            error!(
                device_id,
                "WiFi scan failed on device; preserving existing scan results"
            );
        }
        SafeCommandRx::UpdateSystemInfo { ref system_info } => {
            sqlx::query!(
                "UPDATE device SET system_info = $2 WHERE id = $1",
                device_id,
                system_info
            )
            .execute(&mut *tx)
            .await?;
            // LAN membership is secondary; never let it fail the device's check-in.
            if let Err(err) =
                crate::lan::record_interface_addresses(&mut tx, device_id, system_info).await
            {
                error!(device_id, "Failed to record interface addresses: {err:?}");
            }
        }
        SafeCommandRx::TestNetwork {
            bytes_downloaded,
            duration_ms,
            bytes_uploaded,
            upload_duration_ms,
            timed_out,
        } => {
            // Record results if we have valid data (even partial results from timeout)
            if duration_ms > 0 {
                let download_speed_mbps =
                    (bytes_downloaded as f64 * 8.0) / (duration_ms as f64 * 1000.0);

                let upload_speed_mbps = match (bytes_uploaded, upload_duration_ms) {
                    (Some(bytes), Some(duration)) if duration > 0 => {
                        Some((bytes as f64 * 8.0) / (duration as f64 * 1000.0))
                    }
                    _ => None,
                };

                // If the test timed out, the network is too slow - cap score at 1
                let network_score = if timed_out {
                    1
                } else if download_speed_mbps >= 50.0 {
                    5
                } else if download_speed_mbps >= 25.0 {
                    4
                } else if download_speed_mbps >= 10.0 {
                    3
                } else if download_speed_mbps >= 5.0 {
                    2
                } else {
                    1
                };

                sqlx::query!(
                            "INSERT INTO device_network (device_id, network_score, download_speed_mbps, upload_speed_mbps, source, updated_at)
                            VALUES ($1, $2, $3, $4, $5, NOW())
                            ON CONFLICT (device_id)
                            DO UPDATE SET
                                network_score = EXCLUDED.network_score,
                                download_speed_mbps = EXCLUDED.download_speed_mbps,
                                upload_speed_mbps = EXCLUDED.upload_speed_mbps,
                                source = EXCLUDED.source,
                                updated_at = NOW()",
                            device_id,
                            network_score,
                            download_speed_mbps,
                            upload_speed_mbps,
                            "speed_test"
                        )
                        .execute(&mut *tx)
                        .await?;
            }
        }
        SafeCommandRx::ExtendedNetworkTest { ref samples, .. } => {
            if !samples.is_empty() {
                let avg_download =
                    samples.iter().map(|s| s.download_mbps).sum::<f64>() / samples.len() as f64;

                let upload_samples: Vec<f64> =
                    samples.iter().filter_map(|s| s.upload_mbps).collect();
                let avg_upload = if upload_samples.is_empty() {
                    None
                } else {
                    Some(upload_samples.iter().sum::<f64>() / upload_samples.len() as f64)
                };

                let network_score = if avg_download >= 50.0 {
                    5
                } else if avg_download >= 25.0 {
                    4
                } else if avg_download >= 10.0 {
                    3
                } else if avg_download >= 5.0 {
                    2
                } else {
                    1
                };

                sqlx::query!(
                        "INSERT INTO device_network (device_id, network_score, download_speed_mbps, upload_speed_mbps, source, updated_at)
                        VALUES ($1, $2, $3, $4, $5, NOW())
                        ON CONFLICT (device_id)
                        DO UPDATE SET
                            network_score = EXCLUDED.network_score,
                            download_speed_mbps = EXCLUDED.download_speed_mbps,
                            upload_speed_mbps = EXCLUDED.upload_speed_mbps,
                            source = EXCLUDED.source,
                            updated_at = NOW()",
                        device_id,
                        network_score,
                        avg_download,
                        avg_upload,
                        "extended_test"
                    )
                    .execute(&mut *tx)
                    .await?;
            }
        }
        SafeCommandRx::AuditReport {
            disk_encrypted,
            password_access_disabled,
        } => {
            sqlx::query!(
                r#"
        INSERT INTO device_audit (device_id, disk_encrypted, password_access_disabled, checked_at)
        VALUES ($1, $2, $3, NOW())
        ON CONFLICT (device_id) DO UPDATE SET
            disk_encrypted = EXCLUDED.disk_encrypted,
            password_access_disabled = EXCLUDED.password_access_disabled,
            checked_at = EXCLUDED.checked_at
        "#,
                device_id,
                disk_encrypted,
                password_access_disabled,
            )
            .execute(&mut *tx)
            .await?;
        }
        SafeCommandRx::ApplyNetworksResult {
            applied_version,
            ref conditions,
        } => {
            if response.status == 0 {
                sqlx::query!(
                        "UPDATE device SET observed_intent_version = $2, network_conditions = $3 WHERE id = $1",
                        device_id,
                        applied_version,
                        json!(conditions)
                    )
                    .execute(&mut *tx)
                    .await?;
            }
        }
        SafeCommandRx::ReportSettings { ref report }
        | SafeCommandRx::ApplySettingsResult { ref report, .. }
            if response.status == 0 =>
        {
            // Display only: a bad report must not drop the rest of the poll.
            if let Err(err) = save_settings_report(&mut tx, device_id, report).await {
                error!(device_id, "Failed to store settings report: {err:?}");
            }
        }
        _ => {}
    }
    let mut response_json = match &response.command {
        SafeCommandRx::ReportNMProfiles { profiles } => {
            // Serialize the whole profile so any future NMProfile field is kept
            // automatically, then redact just the secret.
            let redacted: Vec<_> = profiles
                .iter()
                .map(|p| {
                    let mut v = json!(p);
                    if let Some(obj) = v.as_object_mut() {
                        obj.insert("password".to_string(), Value::Null);
                    }
                    v
                })
                .collect();
            json!({ "ReportNMProfiles": { "profiles": redacted } })
        }
        other => json!(other),
    };
    sanitize_nul(&mut response_json);
    let _response_id = sqlx::query_scalar!(
        "INSERT INTO command_response (device_id, command_id, response, status)
                VALUES (
                    $1,
                    CASE WHEN $2 < 0 THEN NULL ELSE $2 END,
                    $3::jsonb,
                    $4
                )
                RETURNING id",
        device_id,
        response.id,
        response_json,
        response.status
    )
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(true)
}

pub async fn get_commands(
    device_id: i32,
    device_serial_number: &str,
    pool: &PgPool,
) -> Result<Vec<SafeCommandRequest>, sqlx::Error> {
    // Single atomic CTE avoids explicit BEGIN/COMMIT round trips on every poll
    // while keeping FOR UPDATE serialization against concurrent queue_latest refreshes.
    let fetched_commands: Vec<CommandsDB> = sqlx::query_as!(
        CommandsDB,
        r#"
            WITH fetched AS (
                SELECT
                    id,
                    cmd,
                    continue_on_error
                FROM command_queue
                WHERE device_id = $1
                  AND fetched = false
                  AND canceled = false
                  AND available_at <= now()
                ORDER BY id
                LIMIT $2
                FOR UPDATE
            ),
            marked AS (
                UPDATE command_queue
                SET fetched_at = CURRENT_TIMESTAMP, fetched = true
                WHERE id IN (SELECT id FROM fetched)
                RETURNING id, cmd, continue_on_error
            )
            SELECT
                id AS "id!",
                cmd AS "cmd!",
                continue_on_error AS "continue_on_error!"
            FROM marked
            ORDER BY id
        "#,
        device_id,
        COMMANDS_PER_ANSWER
    )
    .fetch_all(pool)
    .await?;

    Ok(fetched_commands
        .into_iter()
        .filter_map(|cmd| match serde_json::from_value(cmd.cmd) {
            Ok(command) => Some(SafeCommandRequest {
                id: cmd.id,
                command,
                continue_on_error: cmd.continue_on_error,
            }),
            Err(err) => {
                error!(
                    serial_number = device_serial_number,
                    cmd_id = cmd.id,
                    "Failed to deserialize command from database: {err}"
                );
                None
            }
        })
        .collect())
}

/// Refreshes `checked_at` periodically even when a service stays healthy so
/// dashboard freshness timestamps stay bounded without rewriting rows every 20 s.
const SERVICE_STATUS_HEARTBEAT_SECS: f64 = 300.0;

/// Batches `device_service_status` upserts and `device_service_outage` transitions
/// into one atomic statement.
///
/// Only `"failed"` and `"inactive"` open an outage; everything else (such as
/// `"unknown"` when `systemctl show` times out) is indeterminate and never opens
/// or closes an outage. `smithd` reachability outages belong to the sweeper alone.
pub async fn save_service_statuses(
    device_id: i32,
    statuses: &[ServiceStatus],
    pool: &PgPool,
) -> Result<()> {
    if statuses.is_empty() {
        return Ok(());
    }

    // Deduplicate by release_service_id (last wins) so a single ON CONFLICT DO
    // UPDATE cannot target the same key twice.
    let mut deduped: Vec<&ServiceStatus> = Vec::with_capacity(statuses.len());
    for status in statuses {
        if let Some(slot) = deduped.iter_mut().find(|s| s.id == status.id) {
            *slot = status;
        } else {
            deduped.push(status);
        }
    }

    let service_ids: Vec<i32> = deduped.iter().map(|s| s.id).collect();
    let active_states: Vec<String> = deduped.iter().map(|s| s.active_state.clone()).collect();
    let n_restarts: Vec<i32> = deduped.iter().map(|s| s.n_restarts as i32).collect();

    // Join release_services up front so deleted service ids from a replica cache
    // are skipped instead of aborting the batch with a foreign key error. Unchanged
    // rows within the heartbeat window produce zero heap updates or WAL.
    sqlx::query!(
        r#"
        WITH valid_input AS (
            SELECT i.release_service_id, i.active_state, i.n_restarts, rs.service_name
            FROM UNNEST($2::int4[], $3::text[], $4::int4[])
                AS i(release_service_id, active_state, n_restarts)
            JOIN release_services rs ON rs.id = i.release_service_id
        ),
        upsert AS (
            INSERT INTO device_service_status
                (device_id, release_service_id, active_state, n_restarts, checked_at)
            SELECT $1, vi.release_service_id, vi.active_state, vi.n_restarts, NOW()
            FROM valid_input vi
            ON CONFLICT (device_id, release_service_id)
            DO UPDATE SET active_state = EXCLUDED.active_state,
                          n_restarts   = EXCLUDED.n_restarts,
                          checked_at   = NOW()
            WHERE device_service_status.active_state IS DISTINCT FROM EXCLUDED.active_state
               OR device_service_status.n_restarts   IS DISTINCT FROM EXCLUDED.n_restarts
               OR device_service_status.checked_at   <  NOW() - make_interval(secs => $5::double precision)
            RETURNING release_service_id, active_state
        ),
        written AS (
            SELECT u.active_state, vi.service_name
            FROM upsert u
            JOIN valid_input vi ON vi.release_service_id = u.release_service_id
            WHERE vi.service_name <> $6
        ),
        open_outages AS (
            INSERT INTO device_service_outage (device_id, service_name, started_at)
            SELECT DISTINCT $1, w.service_name, NOW()
            FROM written w
            WHERE w.active_state IN ('failed', 'inactive')
            ON CONFLICT DO NOTHING
        ),
        close_outages AS (
            UPDATE device_service_outage o
            SET ended_at = NOW()
            FROM (
                SELECT DISTINCT service_name
                FROM written
                WHERE active_state = 'active'
                  AND service_name NOT IN (
                      SELECT service_name
                      FROM written
                      WHERE active_state IN ('failed', 'inactive')
                  )
            ) w
            WHERE o.device_id = $1
              AND o.service_name = w.service_name
              AND o.ended_at IS NULL
        )
        SELECT COUNT(*) FROM upsert
        "#,
        device_id,
        &service_ids,
        &active_states,
        &n_restarts,
        SERVICE_STATUS_HEARTBEAT_SECS,
        SMITHD_SERVICE_NAME,
    )
    .fetch_one(pool)
    .await?;

    Ok(())
}

pub async fn add_commands(
    serial_number: &str,
    commands: Vec<SafeCommandRequest>,
    pool: &PgPool,
    user_id: Option<i32>,
) -> Result<Vec<i32>> {
    debug!("Adding commands to device {}", serial_number);
    debug!("Commands: {:?}", commands);
    let mut tx = pool.begin().await?;
    let command_ids = insert_commands(&mut tx, serial_number, commands, user_id).await?;
    tx.commit().await?;
    Ok(command_ids)
}

/// Refreshes a pending automatic command in the caller's transaction; user commands stay.
async fn queue_latest(
    conn: &mut PgConnection,
    device_id: i32,
    serial_number: &str,
    request: SafeCommandRequest,
) -> Result<()> {
    // The pending row may not exist yet, so serialize refresh-or-insert by device.
    sqlx::query("SELECT pg_advisory_xact_lock($1, $2)")
        .bind(SYSTEM_COMMAND_LOCK)
        .bind(device_id)
        .execute(&mut *conn)
        .await?;
    let refreshed = sqlx::query!(
        "UPDATE command_queue SET cmd = $2::jsonb
         WHERE device_id = $1 AND fetched = false AND canceled = false
           AND cmd::jsonb ? (SELECT jsonb_object_keys($2::jsonb))
           AND available_at <= now()
           AND bundle IN (SELECT uuid FROM command_bundles WHERE user_id IS NULL)",
        device_id,
        json!(request.command)
    )
    .execute(&mut *conn)
    .await?
    .rows_affected();
    if refreshed == 0 {
        insert_commands(conn, serial_number, vec![request], None).await?;
    }
    Ok(())
}

/// Queues commands in the caller's transaction.
async fn insert_commands(
    conn: &mut PgConnection,
    serial_number: &str,
    commands: Vec<SafeCommandRequest>,
    user_id: Option<i32>,
) -> Result<Vec<i32>> {
    let mut command_ids = Vec::new();
    let bundle_id = sqlx::query!(
        r#"INSERT INTO command_bundles (user_id) VALUES ($1) RETURNING uuid"#,
        user_id
    )
    .fetch_one(&mut *conn)
    .await?;

    for command in commands {
        let command_id: i32 = sqlx::query_scalar!(
            "INSERT INTO command_queue (device_id, cmd, continue_on_error, canceled, bundle)
                VALUES (
                    (SELECT id FROM device WHERE serial_number = $1),
                    $2::jsonb,
                    $3,
                    false,
                    $4
                )
                RETURNING id;",
            serial_number,
            json!(command.command),
            command.continue_on_error,
            bundle_id.uuid
        )
        .fetch_one(&mut *conn)
        .await?;

        command_ids.push(command_id);
    }
    Ok(command_ids)
}

/// Replaces the device's stored report. JSON keys map to columns by name
/// (`jsonb_populate_record`); `settings_report_matches_the_table` keeps the
/// struct and the table in step; `apply_status` has no column and goes to
/// `device_setting_apply_status`. Inside an open transaction `begin` makes this
/// a savepoint, so a failure rolls back only this report.
pub async fn save_settings_report(
    conn: &mut PgConnection,
    device_id: i32,
    report: &schema::SettingsReport,
) -> Result<()> {
    let statuses = serde_json::to_value(&report.apply_status)?;
    let report = serde_json::to_value(report)?;
    let mut tx = conn.begin().await?;
    sqlx::query!(
        "DELETE FROM device_settings_reported WHERE device_id = $1",
        device_id
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        r#"
        INSERT INTO device_settings_reported
        SELECT * FROM jsonb_populate_record(
            NULL::device_settings_reported,
            $2::jsonb || jsonb_build_object('device_id', $1::int4, 'reported_at', NOW())
        )
        "#,
        device_id,
        report,
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        "DELETE FROM device_setting_apply_status WHERE device_id = $1",
        device_id
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        r#"
        INSERT INTO device_setting_apply_status (device_id, setting, state, reason)
        SELECT $1, s.setting, s.state, s.reason
        FROM jsonb_populate_recordset(NULL::device_setting_apply_status, $2::jsonb) s
        "#,
        device_id,
        statuses,
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        COMMAND_RESULT_LOCK, Saved, get_commands, map_key_mgmt, queue_latest, save_responses,
        save_service_statuses, save_settings_report, schema,
    };
    use schema::{HomePost, SafeCommandRequest, SafeCommandRx, SafeCommandTx, ServiceStatus};
    use serde_json::{Value, json};
    use sqlx::PgPool;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::time::Duration;

    #[test]
    fn map_key_mgmt_known_values() {
        assert_eq!(map_key_mgmt("open"), Some("open"));
        assert_eq!(map_key_mgmt("none"), Some("open"));
        assert_eq!(map_key_mgmt("wpa-psk"), Some("wpa-psk"));
        assert_eq!(map_key_mgmt("sae"), Some("sae"));
        assert_eq!(map_key_mgmt("owe"), Some("owe"));
        assert_eq!(map_key_mgmt("wpa-eap"), Some("wpa-eap"));
    }

    #[test]
    fn map_key_mgmt_unknown_returns_none() {
        assert_eq!(map_key_mgmt("wep"), None);
        assert_eq!(map_key_mgmt(""), None);
        assert_eq!(map_key_mgmt("wpa3"), None);
    }

    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn settings_report_replaces_the_previous_one(pool: PgPool) -> anyhow::Result<()> {
        let device_id: i32 =
            sqlx::query_scalar("INSERT INTO device (serial_number) VALUES ('s') RETURNING id")
                .fetch_one(&pool)
                .await?;
        let first = schema::SettingsReport {
            wireless_mode: Some(11),
            hostname: Some("a".to_owned()),
            ..Default::default()
        };
        let second = schema::SettingsReport {
            hostname: Some("b".to_owned()),
            ..Default::default()
        };
        let mut conn = pool.acquire().await?;
        save_settings_report(&mut conn, device_id, &first).await?;
        save_settings_report(&mut conn, device_id, &second).await?;

        let rows: Vec<(Option<i16>, Option<String>)> = sqlx::query_as(
            "SELECT wireless_mode, hostname FROM device_settings_reported WHERE device_id = $1",
        )
        .bind(device_id)
        .fetch_all(&pool)
        .await?;
        assert_eq!(rows, vec![(None, Some("b".to_owned()))]);
        Ok(())
    }

    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn settings_report_stores_apply_fields_and_replaces_statuses(
        pool: PgPool,
    ) -> anyhow::Result<()> {
        let device_id: i32 =
            sqlx::query_scalar("INSERT INTO device (serial_number) VALUES ('s') RETURNING id")
                .fetch_one(&pool)
                .await?;
        let failed = schema::SettingStatus {
            setting: schema::DeviceSetting::WirelessMode,
            state: schema::SettingApplyState::Failed,
            reason: "still 95 after the reload".to_owned(),
        };
        let first = schema::SettingsReport {
            ap_country_code: Some("DK".to_owned()),
            applied_version: Some(3),
            corrected: vec![schema::DeviceSetting::IdleTh],
            apply_status: vec![failed],
            ..Default::default()
        };
        let mut conn = pool.acquire().await?;
        save_settings_report(&mut conn, device_id, &first).await?;

        let row: (Option<String>, Option<i32>, Vec<String>) = sqlx::query_as(
            "SELECT ap_country_code, applied_version, corrected::text[]
             FROM device_settings_reported WHERE device_id = $1",
        )
        .bind(device_id)
        .fetch_one(&pool)
        .await?;
        assert_eq!(
            row,
            (Some("DK".to_owned()), Some(3), vec!["idle_th".to_owned()])
        );
        let statuses: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT setting::text, state::text, reason FROM device_setting_apply_status
             WHERE device_id = $1",
        )
        .bind(device_id)
        .fetch_all(&pool)
        .await?;
        assert_eq!(
            statuses,
            vec![(
                "wireless_mode".to_owned(),
                "failed".to_owned(),
                "still 95 after the reload".to_owned()
            )]
        );

        // A report from a smithd without these fields clears them.
        save_settings_report(&mut conn, device_id, &schema::SettingsReport::default()).await?;
        let statuses: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM device_setting_apply_status WHERE device_id = $1",
        )
        .bind(device_id)
        .fetch_one(&pool)
        .await?;
        assert_eq!(statuses, 0);
        Ok(())
    }

    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn apply_settings_results_store_the_report(pool: PgPool) -> anyhow::Result<()> {
        use schema::{HomePost, SafeCommandResponse, SafeCommandRx, SettingsReport};

        let device_id: i32 =
            sqlx::query_scalar("INSERT INTO device (serial_number) VALUES ('r') RETURNING id")
                .fetch_one(&pool)
                .await?;
        let post = |command: SafeCommandRx, status: i32| {
            HomePost::new(
                vec![SafeCommandResponse {
                    id: -1,
                    command,
                    status,
                }],
                None,
                vec![],
            )
        };
        let stored = |pool: PgPool| async move {
            sqlx::query_as::<_, (Option<i32>, Option<String>)>(
                "SELECT applied_version, hostname FROM device_settings_reported WHERE device_id = $1",
            )
            .bind(device_id)
            .fetch_optional(&pool)
            .await
        };

        // Applied: the report is stored as smithd sent it.
        let applied = SafeCommandRx::ApplySettingsResult {
            applied_version: 2,
            reverted: false,
            report: Box::new(SettingsReport {
                applied_version: Some(2),
                hostname: Some("a".to_owned()),
                ..Default::default()
            }),
        };
        save_responses(device_id, "r", post(applied, 0), &pool).await?;
        assert_eq!(
            stored(pool.clone()).await?,
            Some((Some(2), Some("a".to_owned())))
        );

        // A failed read is answered with an empty report: nothing is stored.
        let failed = SafeCommandRx::ApplySettingsResult {
            applied_version: 2,
            reverted: false,
            report: Box::default(),
        };
        save_responses(device_id, "r", post(failed, -1), &pool).await?;
        assert_eq!(
            stored(pool.clone()).await?,
            Some((Some(2), Some("a".to_owned())))
        );

        Ok(())
    }

    async fn insert_device(pool: &PgPool, serial: &str) -> anyhow::Result<i32> {
        Ok(
            sqlx::query_scalar("INSERT INTO device (serial_number) VALUES ($1) RETURNING id")
                .bind(serial)
                .fetch_one(pool)
                .await?,
        )
    }

    async fn insert_get_variables(pool: &PgPool, device_id: i32) -> anyhow::Result<i32> {
        Ok(sqlx::query_scalar(
            "WITH bundle AS (INSERT INTO command_bundles DEFAULT VALUES RETURNING uuid)
             INSERT INTO command_queue (device_id, cmd, continue_on_error, canceled, bundle)
             SELECT $1, '\"GetVariables\"'::jsonb, false, false, uuid FROM bundle RETURNING id",
        )
        .bind(device_id)
        .fetch_one(pool)
        .await?)
    }

    // GetVariables queues an UpdateVariables: the side effect a resend must not repeat.
    fn get_variables_result(command_id: i32) -> HomePost {
        HomePost::new(
            vec![schema::SafeCommandResponse {
                id: command_id,
                command: SafeCommandRx::GetVariables,
                status: 0,
            }],
            None,
            vec![],
        )
    }

    /// Response rows of `device_id` for the command, and its queued UpdateVariables.
    async fn stored(pool: &PgPool, device_id: i32, command_id: i32) -> anyhow::Result<(i64, i64)> {
        Ok(sqlx::query_as(
            "SELECT
                 (SELECT count(*) FROM command_response WHERE command_id = $2 AND device_id = $1),
                 (SELECT count(*) FROM command_queue WHERE device_id = $1 AND cmd::jsonb ? 'UpdateVariables')",
        )
        .bind(device_id)
        .bind(command_id)
        .fetch_one(pool)
        .await?)
    }

    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn a_result_sent_again_is_stored_once_and_its_side_effects_run_once(
        pool: PgPool,
    ) -> anyhow::Result<()> {
        let device_id = insert_device(&pool, "d").await?;
        let command_id = insert_get_variables(&pool, device_id).await?;
        save_responses(device_id, "d", get_variables_result(command_id), &pool).await?;
        save_responses(device_id, "d", get_variables_result(command_id), &pool).await?;
        assert_eq!(stored(&pool, device_id, command_id).await?, (1, 1));
        Ok(())
    }

    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn get_variables_sent_again_refreshes_the_unfetched_update(
        pool: PgPool,
    ) -> anyhow::Result<()> {
        let device_id = insert_device(&pool, "d").await?;
        save_responses(device_id, "d", get_variables_result(-1), &pool).await?;
        sqlx::query("INSERT INTO variable (device, name, value) VALUES ($1, 'k', 'v')")
            .bind(device_id)
            .execute(&pool)
            .await?;
        save_responses(device_id, "d", get_variables_result(-1), &pool).await?;
        let queued: Vec<serde_json::Value> = sqlx::query_scalar(
            "SELECT cmd::jsonb FROM command_queue WHERE device_id = $1 AND cmd::jsonb ? 'UpdateVariables'",
        )
        .bind(device_id)
        .fetch_all(&pool)
        .await?;
        assert_eq!(
            queued,
            vec![serde_json::json!({"UpdateVariables": {"variables": {"k": "v"}}})]
        );
        Ok(())
    }

    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn overlapping_system_updates_refresh_one_pending_command(
        pool: PgPool,
    ) -> anyhow::Result<()> {
        let device_id = insert_device(&pool, "d").await?;
        let request = |value: &str| SafeCommandRequest {
            id: -1,
            command: SafeCommandTx::UpdateVariables {
                variables: [("mode".to_owned(), value.to_owned())].into(),
            },
            continue_on_error: false,
        };
        let mut first = pool.begin().await?;
        queue_latest(&mut first, device_id, "d", request("first")).await?;
        let mut second = pool.begin().await?;
        {
            let refresh = queue_latest(&mut second, device_id, "d", request("latest"));
            tokio::pin!(refresh);
            assert!(
                tokio::time::timeout(Duration::from_millis(50), &mut refresh)
                    .await
                    .is_err()
            );
            first.commit().await?;
            refresh.await?;
        }
        second.commit().await?;
        let queued: Vec<Value> =
            sqlx::query_scalar("SELECT cmd::jsonb FROM command_queue WHERE device_id = $1")
                .bind(device_id)
                .fetch_all(&pool)
                .await?;
        assert_eq!(
            queued,
            vec![json!({"UpdateVariables": {"variables": {"mode": "latest"}}})]
        );
        Ok(())
    }

    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn a_resend_during_the_first_post_does_not_wait_and_is_stored_once(
        pool: PgPool,
    ) -> anyhow::Result<()> {
        let device_id = insert_device(&pool, "d").await?;
        let command_id = insert_get_variables(&pool, device_id).await?;
        // The first post, still running: it holds the lock and has not committed its row.
        let mut first = pool.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock($1, $2)")
            .bind(COMMAND_RESULT_LOCK)
            .bind(command_id)
            .execute(&mut *first)
            .await?;
        sqlx::query(
            "INSERT INTO command_response (device_id, command_id, response, status)
             VALUES ($1, $2, '\"GetVariables\"'::jsonb, 0)",
        )
        .bind(device_id)
        .bind(command_id)
        .execute(&mut *first)
        .await?;

        let resend = tokio::time::timeout(
            Duration::from_secs(1),
            save_responses(device_id, "d", get_variables_result(command_id), &pool),
        )
        .await??;
        assert_eq!(resend, Saved::Partly, "the resend is told to send again");
        first.commit().await?;
        let again = save_responses(device_id, "d", get_variables_result(command_id), &pool).await?;
        assert_eq!(again, Saved::All);

        assert_eq!(stored(&pool, device_id, command_id).await?, (1, 0));
        Ok(())
    }

    fn profile(ssid: &str) -> schema::NMProfile {
        schema::NMProfile {
            name: ssid.to_owned(),
            ssid: Some(ssid.to_owned()),
            password: Some("secret".to_owned()),
            is_active: ssid == "site-a",
            key_mgmt: Some("wpa-psk".to_owned()),
            ..Default::default()
        }
    }

    /// One poll of a device on a shared site: a command result, a system info
    /// snapshot and the site's SSIDs in a device specific order.
    fn site_post(device: usize, command_id: i32) -> HomePost {
        let mut ssids = vec!["site-a", "site-b", "site-c"];
        ssids.rotate_left(device % 3);
        HomePost::new(
            vec![
                schema::SafeCommandResponse {
                    id: command_id,
                    command: SafeCommandRx::GetVariables,
                    status: 0,
                },
                schema::SafeCommandResponse {
                    id: -2,
                    command: SafeCommandRx::UpdateSystemInfo {
                        system_info: serde_json::json!({}),
                    },
                    status: 0,
                },
                schema::SafeCommandResponse {
                    id: -6,
                    command: SafeCommandRx::ReportNMProfiles {
                        profiles: ssids.into_iter().map(profile).collect(),
                    },
                    status: 0,
                },
            ],
            None,
            vec![],
        )
    }

    // The 2026-11-03 outage: many devices of one site post at once, each sends
    // the same results again, and the pool is far smaller than the posts in
    // flight. Each post must finish before smithd's 10 s timeout, without a
    // pool timeout, and every result must be stored once.
    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn concurrent_resends_on_a_small_pool_finish_and_store_each_result_once(
        pool_options: PgPoolOptions,
        connect_options: PgConnectOptions,
    ) -> anyhow::Result<()> {
        const DEVICES: usize = 40;
        const CONNECTIONS: u32 = 4;
        let pool = pool_options
            .max_connections(CONNECTIONS)
            .acquire_timeout(Duration::from_secs(3))
            .connect_with(connect_options)
            .await?;
        let mut devices = Vec::new();
        for device in 0..DEVICES {
            let device_id = insert_device(&pool, &format!("d{device}")).await?;
            devices.push((
                device,
                device_id,
                insert_get_variables(&pool, device_id).await?,
            ));
        }

        let mut pending = devices.clone();
        for round in 0..5 {
            if pending.is_empty() {
                break;
            }
            // First round: each device posts twice at once, the post it gave up
            // on and its resend. Later rounds: one resend per poll told to retry.
            let copies = if round == 0 { 2 } else { 1 };
            let mut posts = Vec::new();
            for &(device, device_id, command_id) in &pending {
                for _ in 0..copies {
                    let pool = pool.clone();
                    posts.push(tokio::spawn(async move {
                        let serial = format!("d{device}");
                        let saved = tokio::time::timeout(
                            Duration::from_secs(10),
                            save_responses(
                                device_id,
                                &serial,
                                site_post(device, command_id),
                                &pool,
                            ),
                        )
                        .await;
                        (device, device_id, command_id, saved)
                    }));
                }
            }
            let mut again = Vec::new();
            for post in posts {
                let (device, device_id, command_id, saved) = post.await?;
                let saved = saved.map_err(|_| anyhow::anyhow!("a post ran past 10 s"))??;
                if saved == Saved::Partly && !again.contains(&(device, device_id, command_id)) {
                    again.push((device, device_id, command_id));
                }
            }
            pending = again;
        }
        assert!(pending.is_empty(), "results still not stored: {pending:?}");

        for (_, device_id, command_id) in devices {
            assert_eq!(stored(&pool, device_id, command_id).await?, (1, 1));
        }
        let networks: i64 =
            sqlx::query_scalar("SELECT count(*) FROM network WHERE ssid LIKE 'site-%'")
                .fetch_one(&pool)
                .await?;
        assert_eq!(
            networks, 3,
            "devices that share SSIDs converge to one row each"
        );
        Ok(())
    }

    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn a_result_under_another_devices_command_does_not_hide_the_real_one(
        pool: PgPool,
    ) -> anyhow::Result<()> {
        let device_id = insert_device(&pool, "d").await?;
        let other = insert_device(&pool, "o").await?;
        let command_id = insert_get_variables(&pool, device_id).await?;
        save_responses(other, "o", get_variables_result(command_id), &pool).await?;
        save_responses(device_id, "d", get_variables_result(command_id), &pool).await?;
        assert_eq!(stored(&pool, device_id, command_id).await?, (1, 1));
        Ok(())
    }

    fn nm_profiles_post(device: usize, id: i32) -> HomePost {
        let mut ssids = vec!["site-a", "site-b", "site-c"];
        ssids.rotate_left(device % 3);
        HomePost::new(
            vec![schema::SafeCommandResponse {
                id,
                command: SafeCommandRx::ReportNMProfiles {
                    profiles: ssids.into_iter().map(profile).collect(),
                },
                status: 0,
            }],
            None,
            vec![],
        )
    }

    async fn configured_networks(pool: &PgPool, device_id: i32) -> anyhow::Result<i64> {
        Ok(sqlx::query_scalar(
            "SELECT count(*) FROM device_configured_network WHERE device_id = $1",
        )
        .bind(device_id)
        .fetch_one(pool)
        .await?)
    }

    // Shorter than lock_timeout, so a post that waits on a lock fails.
    const NO_LOCK_WAIT: Duration = Duration::from_millis(900);

    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn a_busy_network_lock_skips_a_snapshot_and_resends_a_command_result(
        pool: PgPool,
    ) -> anyhow::Result<()> {
        let device_id = insert_device(&pool, "d").await?;
        // Another device of the site stores the same network now.
        let mut other = pool.begin().await?;
        sqlx::query(
            r#"SELECT pg_advisory_xact_lock(
                   network_content_lock_key('site-b', false, '{"psk": "secret"}'::jsonb))"#,
        )
        .execute(&mut *other)
        .await?;

        let snapshot = tokio::time::timeout(
            NO_LOCK_WAIT,
            save_responses(device_id, "d", nm_profiles_post(0, -6), &pool),
        )
        .await??;
        assert_eq!(snapshot, Saved::All, "a skipped snapshot is not sent again");
        assert_eq!(configured_networks(&pool, device_id).await?, 0);

        let answer = tokio::time::timeout(
            NO_LOCK_WAIT,
            save_responses(device_id, "d", nm_profiles_post(0, 7), &pool),
        )
        .await??;
        assert_eq!(answer, Saved::Partly, "a command result is sent again");

        other.commit().await?;
        let again = save_responses(device_id, "d", nm_profiles_post(0, -6), &pool).await?;
        assert_eq!(again, Saved::All);
        assert_eq!(configured_networks(&pool, device_id).await?, 3);
        Ok(())
    }

    // Many devices of one site report the same SSIDs at once on a small pool:
    // no post waits on another device's network lock or is told to retry.
    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn concurrent_nm_profile_snapshots_of_a_site_do_not_wait_and_converge(
        pool_options: PgPoolOptions,
        connect_options: PgConnectOptions,
    ) -> anyhow::Result<()> {
        const DEVICES: usize = 40;
        let pool = pool_options
            .max_connections(4)
            .acquire_timeout(Duration::from_secs(3))
            .connect_with(connect_options)
            .await?;
        let mut device_ids = Vec::new();
        for device in 0..DEVICES {
            device_ids.push(insert_device(&pool, &format!("d{device}")).await?);
        }

        let mut posts = Vec::new();
        for (device, &device_id) in device_ids.iter().enumerate() {
            let pool = pool.clone();
            posts.push(tokio::spawn(async move {
                let started = tokio::time::Instant::now();
                let saved =
                    save_responses(device_id, "d", nm_profiles_post(device, -6), &pool).await;
                (saved, started.elapsed())
            }));
        }
        for post in posts {
            let (saved, elapsed) = post.await?;
            assert_eq!(saved?, Saved::All);
            assert!(elapsed < Duration::from_secs(3), "a post took {elapsed:?}");
        }

        let networks: i64 =
            sqlx::query_scalar("SELECT count(*) FROM network WHERE ssid LIKE 'site-%'")
                .fetch_one(&pool)
                .await?;
        assert_eq!(
            networks, 3,
            "devices that share SSIDs converge to one row each"
        );
        let reporting: i64 =
            sqlx::query_scalar("SELECT count(DISTINCT device_id) FROM device_configured_network")
                .fetch_one(&pool)
                .await?;
        assert!(reporting >= 1, "at least one snapshot is stored");
        Ok(())
    }

    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn settings_report_matches_the_table(pool: PgPool) -> anyhow::Result<()> {
        use std::collections::BTreeSet;

        let report = serde_json::to_value(schema::SettingsReport::default())?;
        let fields: BTreeSet<String> = report
            .as_object()
            .map(|o| o.keys().cloned().collect())
            .unwrap_or_default();

        let columns: BTreeSet<String> = sqlx::query_scalar(
            "SELECT column_name::text FROM information_schema.columns
             WHERE table_name = 'device_settings_reported'",
        )
        .fetch_all(&pool)
        .await?
        .into_iter()
        .collect();
        let server_side: BTreeSet<String> = ["device_id", "reported_at"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        let own_table: BTreeSet<String> = ["apply_status".to_owned()].into_iter().collect();
        assert_eq!(
            &fields - &own_table,
            &columns - &server_side,
            "SettingsReport fields and device_settings_reported columns differ"
        );

        let enum_values: BTreeSet<String> =
            sqlx::query_scalar("SELECT unnest(enum_range(NULL::device_setting))::text")
                .fetch_all(&pool)
                .await?
                .into_iter()
                .collect();
        let not_settings = [
            "read_errors",
            "applied_version",
            "apply_status",
            "corrected",
        ];
        let settings: BTreeSet<String> = fields
            .into_iter()
            .filter(|field| !not_settings.contains(&field.as_str()))
            .collect();
        assert_eq!(
            enum_values, settings,
            "device_setting enum and SettingsReport fields differ"
        );
        for value in &enum_values {
            serde_json::from_value::<schema::DeviceSetting>(serde_json::json!(value))?;
        }
        Ok(())
    }

    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn get_commands_marks_pending_rows_atomically_in_id_order(
        pool: PgPool,
    ) -> anyhow::Result<()> {
        let device_id = insert_device(&pool, "cmd-dev").await?;
        let bundle = uuid::Uuid::new_v4();
        sqlx::query("INSERT INTO command_bundles (uuid) VALUES ($1)")
            .bind(bundle)
            .execute(&pool)
            .await?;

        let first_id: i32 = sqlx::query_scalar(
            "INSERT INTO command_queue (device_id, cmd, continue_on_error, bundle)
             VALUES ($1, $2, false, $3) RETURNING id",
        )
        .bind(device_id)
        .bind(serde_json::to_value(schema::SafeCommandTx::Ping)?)
        .bind(bundle)
        .fetch_one(&pool)
        .await?;

        let second_id: i32 = sqlx::query_scalar(
            "INSERT INTO command_queue (device_id, cmd, continue_on_error, bundle)
             VALUES ($1, $2, true, $3) RETURNING id",
        )
        .bind(device_id)
        .bind(serde_json::to_value(schema::SafeCommandTx::Ping)?)
        .bind(bundle)
        .fetch_one(&pool)
        .await?;

        // Canceled and future-scheduled commands must not be returned.
        sqlx::query(
            "INSERT INTO command_queue (device_id, cmd, continue_on_error, canceled, bundle)
             VALUES ($1, $2, false, true, $3)",
        )
        .bind(device_id)
        .bind(serde_json::to_value(schema::SafeCommandTx::Ping)?)
        .bind(bundle)
        .execute(&pool)
        .await?;
        sqlx::query(
            "INSERT INTO command_queue (device_id, cmd, continue_on_error, bundle, available_at)
             VALUES ($1, $2, false, $3, NOW() + INTERVAL '1 hour')",
        )
        .bind(device_id)
        .bind(serde_json::to_value(schema::SafeCommandTx::Ping)?)
        .bind(bundle)
        .execute(&pool)
        .await?;

        let fetched = get_commands(device_id, "cmd-dev", &pool).await?;
        let [first, second] = fetched.as_slice() else {
            anyhow::bail!("expected 2 fetched commands, got {}", fetched.len());
        };
        assert_eq!((first.id, second.id), (first_id, second_id));
        assert!(!first.continue_on_error);
        assert!(second.continue_on_error);

        // Second poll sees zero pending commands.
        assert!(get_commands(device_id, "cmd-dev", &pool).await?.is_empty());

        let marked: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM command_queue
             WHERE id = ANY($1) AND fetched = true AND fetched_at IS NOT NULL",
        )
        .bind(vec![first_id, second_id])
        .fetch_one(&pool)
        .await?;
        assert_eq!(marked, 2);
        Ok(())
    }

    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn save_service_statuses_batches_skips_unchanged_and_tracks_outages(
        pool: PgPool,
    ) -> anyhow::Result<()> {
        let device_id = insert_device(&pool, "svc-dev").await?;
        let dist_id: i32 = sqlx::query_scalar(
            "INSERT INTO distribution (name, description, architecture)
             VALUES ('svc-dist', 'test', 'arm64') RETURNING id",
        )
        .fetch_one(&pool)
        .await?;
        let release_id: i32 = sqlx::query_scalar(
            "INSERT INTO release (distribution_id, version) VALUES ($1, 'v1') RETURNING id",
        )
        .bind(dist_id)
        .fetch_one(&pool)
        .await?;
        let brain_svc: i32 = sqlx::query_scalar(
            "INSERT INTO release_services (release_id, service_name, watchdog_sec)
             VALUES ($1, 'brain.service', 30) RETURNING id",
        )
        .bind(release_id)
        .fetch_one(&pool)
        .await?;
        let smithd_svc: i32 = sqlx::query_scalar(
            "INSERT INTO release_services (release_id, service_name, watchdog_sec)
             VALUES ($1, 'smithd', 30) RETURNING id",
        )
        .bind(release_id)
        .fetch_one(&pool)
        .await?;

        // Duplicate id in one payload: last value ("active") wins.
        save_service_statuses(
            device_id,
            &[
                ServiceStatus {
                    id: brain_svc,
                    active_state: "inactive".into(),
                    n_restarts: 0,
                },
                ServiceStatus {
                    id: brain_svc,
                    active_state: "active".into(),
                    n_restarts: 0,
                },
                ServiceStatus {
                    id: smithd_svc,
                    active_state: "failed".into(),
                    n_restarts: 0,
                },
            ],
            &pool,
        )
        .await?;

        let first_checked: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
            "SELECT checked_at FROM device_service_status
             WHERE device_id = $1 AND release_service_id = $2",
        )
        .bind(device_id)
        .bind(brain_svc)
        .fetch_one(&pool)
        .await?;

        // Identical poll within 5 minutes skips the update so checked_at stays untouched.
        tokio::time::sleep(Duration::from_millis(20)).await;
        save_service_statuses(
            device_id,
            &[ServiceStatus {
                id: brain_svc,
                active_state: "active".into(),
                n_restarts: 0,
            }],
            &pool,
        )
        .await?;
        let second_checked: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
            "SELECT checked_at FROM device_service_status
             WHERE device_id = $1 AND release_service_id = $2",
        )
        .bind(device_id)
        .bind(brain_svc)
        .fetch_one(&pool)
        .await?;
        assert_eq!(second_checked, first_checked);

        // smithd reports must never open an outage row.
        let smithd_outages: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM device_service_outage
             WHERE device_id = $1 AND service_name = 'smithd'",
        )
        .bind(device_id)
        .fetch_one(&pool)
        .await?;
        assert_eq!(smithd_outages, 0);

        // Stale checked_at (> 5 min) refreshes on an identical poll.
        sqlx::query(
            "UPDATE device_service_status SET checked_at = NOW() - INTERVAL '6 minutes'
             WHERE device_id = $1 AND release_service_id = $2",
        )
        .bind(device_id)
        .bind(brain_svc)
        .execute(&pool)
        .await?;
        save_service_statuses(
            device_id,
            &[ServiceStatus {
                id: brain_svc,
                active_state: "active".into(),
                n_restarts: 0,
            }],
            &pool,
        )
        .await?;
        let refreshed_checked: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
            "SELECT checked_at FROM device_service_status
             WHERE device_id = $1 AND release_service_id = $2",
        )
        .bind(device_id)
        .bind(brain_svc)
        .fetch_one(&pool)
        .await?;
        assert!(refreshed_checked > first_checked);

        // Restart increment updates immediately even while active.
        save_service_statuses(
            device_id,
            &[ServiceStatus {
                id: brain_svc,
                active_state: "active".into(),
                n_restarts: 2,
            }],
            &pool,
        )
        .await?;
        let restarts: i32 = sqlx::query_scalar(
            "SELECT n_restarts FROM device_service_status
             WHERE device_id = $1 AND release_service_id = $2",
        )
        .bind(device_id)
        .bind(brain_svc)
        .fetch_one(&pool)
        .await?;
        assert_eq!(restarts, 2);

        // Transition active -> failed opens an outage even when the batch also
        // carries a deleted release_service_id from a stale replica cache.
        save_service_statuses(
            device_id,
            &[
                ServiceStatus {
                    id: 999_999,
                    active_state: "failed".into(),
                    n_restarts: 1,
                },
                ServiceStatus {
                    id: brain_svc,
                    active_state: "failed".into(),
                    n_restarts: 2,
                },
            ],
            &pool,
        )
        .await?;
        let open_count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM device_service_outage
             WHERE device_id = $1 AND service_name = 'brain.service' AND ended_at IS NULL",
        )
        .bind(device_id)
        .fetch_one(&pool)
        .await?;
        assert_eq!(open_count, 1);

        // Indeterminate "unknown" must not close the open outage.
        save_service_statuses(
            device_id,
            &[ServiceStatus {
                id: brain_svc,
                active_state: "unknown".into(),
                n_restarts: 2,
            }],
            &pool,
        )
        .await?;
        let still_open: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM device_service_outage
             WHERE device_id = $1 AND service_name = 'brain.service' AND ended_at IS NULL",
        )
        .bind(device_id)
        .fetch_one(&pool)
        .await?;
        assert_eq!(still_open, 1);

        save_service_statuses(
            device_id,
            &[ServiceStatus {
                id: brain_svc,
                active_state: "active".into(),
                n_restarts: 3,
            }],
            &pool,
        )
        .await?;
        let closed_count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM device_service_outage
             WHERE device_id = $1 AND service_name = 'brain.service' AND ended_at IS NOT NULL",
        )
        .bind(device_id)
        .fetch_one(&pool)
        .await?;
        assert_eq!(closed_count, 1);

        Ok(())
    }
}
