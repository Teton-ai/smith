use crate::device::{SMITHD_SERVICE_NAME, Variable};
use crate::network::route::content_credentials;
use anyhow::Result;
use serde_json::Value;
use serde_json::json;
use smith::utils::schema;
use smith::utils::schema::SafeCommandTx::{UpdateNetwork, UpdateVariables};
use smith::utils::schema::{
    HomePost, NetworkType, SafeCommandRequest, SafeCommandRx, ServiceStatus,
};
use sqlx::PgPool;
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

pub async fn save_responses(
    device_id: i32,
    device_serial_number: &str,
    payload: HomePost,
    pool: &PgPool,
) -> Result<()> {
    let mut tx = pool.begin().await?;

    for response in payload.responses {
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
                add_commands(
                    device_serial_number,
                    vec![SafeCommandRequest {
                        id: -1,
                        command: update_variables,
                        continue_on_error: false,
                    }],
                    pool,
                    None,
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
                    add_commands(
                        device_serial_number,
                        vec![SafeCommandRequest {
                            id: -4,
                            command: UpdateNetwork { network },
                            continue_on_error: false,
                        }],
                        pool,
                        None,
                    )
                    .await?;
                }
            }
            SafeCommandRx::ReportNMProfiles { ref profiles } if response.status == 0 => {
                let mut profiles_resolved: Vec<(i32, bool, String)> = Vec::new();

                // Advisory locks taken here are transaction-scoped, so they are all
                // held until this (long) transaction commits. Taking them in nmcli
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

                for (ssid, hidden, password) in &lock_inputs {
                    let credentials = content_credentials(*password);
                    sqlx::query!(
                        "SELECT pg_advisory_xact_lock(network_content_lock_key($1, $2, $3))",
                        ssid,
                        hidden,
                        credentials,
                    )
                    .execute(&mut *tx)
                    .await?;
                }

                for profile in profiles {
                    let ssid = match profile.ssid.as_deref() {
                        Some(s) => s,
                        None => continue,
                    };

                    let mapped_security_type: Option<&str> =
                        profile.key_mgmt.as_deref().and_then(map_key_mgmt);

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
                                    mapped_security_type as Option<&str>,
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
                                mapped_security_type as Option<&str>,
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
                    let ssids: Vec<Option<String>> =
                        networks.iter().map(|n| n.ssid.clone()).collect();
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
                .execute(pool)
                .await?;
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
                        .execute(pool)
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
                    .execute(pool)
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
                .execute(pool)
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
    }

    tx.commit().await?;
    Ok(())
}

pub async fn get_commands(
    device_id: i32,
    device_serial_number: &str,
    pool: &PgPool,
) -> Result<Vec<SafeCommandRequest>, sqlx::Error> {
    let mut tx = pool.begin().await?;

    let fetched_commands: Vec<CommandsDB> = sqlx::query_as!(
        CommandsDB,
        r#"
            SELECT
                id,
                cmd,
                continue_on_error
            FROM command_queue
            WHERE device_id = $1 AND fetched = false AND canceled = false"#,
        device_id
    )
    .fetch_all(&mut *tx)
    .await?;

    // If commands are fetched successfully, update fetched_at timestamp
    if !fetched_commands.is_empty() {
        let ids: Vec<i32> = fetched_commands.iter().map(|cmd| cmd.id).collect();
        let _ = sqlx::query!(
                    "UPDATE command_queue SET fetched_at = CURRENT_TIMESTAMP, fetched = true WHERE id = ANY($1)",
                    &ids
                )
                .execute(&mut *tx)
                .await;
    }

    tx.commit().await?;

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

/// systemd states that definitely mean a unit is not running. Everything else is
/// indeterminate and must never open an outage — in particular smithd reports the
/// literal `"unknown"` when its `systemctl show` call times out or exits non-zero
/// (`smithd/src/postman/mod.rs`), so treating anything but these as failure would
/// fabricate outages on a merely busy device.
fn is_service_down(active_state: &str) -> bool {
    matches!(active_state, "failed" | "inactive")
}

pub async fn save_service_statuses(
    device_id: i32,
    statuses: &[ServiceStatus],
    pool: &PgPool,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    for status in statuses {
        // The `prev` CTE reads the pre-upsert snapshot, so a single round trip
        // yields both the state being replaced and the service's stable name. A
        // data-modifying CTE always runs to completion whether or not the outer
        // query reads its output, so the upsert still happens.
        let reported = sqlx::query!(
            r#"
            WITH prev AS (
                SELECT active_state FROM device_service_status
                WHERE device_id = $1 AND release_service_id = $2
            ),
            upsert AS (
                INSERT INTO device_service_status
                    (device_id, release_service_id, active_state, n_restarts, checked_at)
                VALUES ($1, $2, $3, $4, NOW())
                ON CONFLICT (device_id, release_service_id)
                DO UPDATE SET active_state = EXCLUDED.active_state,
                              n_restarts  = EXCLUDED.n_restarts,
                              checked_at  = NOW()
                RETURNING 1
            )
            SELECT
                (SELECT active_state FROM prev) AS previous_active_state,
                (SELECT service_name FROM release_services WHERE id = $2) AS service_name
            "#,
            device_id,
            status.id,
            status.active_state,
            status.n_restarts as i32
        )
        .fetch_one(&mut *tx)
        .await?;

        // A release_service row the API no longer has cannot be attributed a
        // name, and reachability rows belong to the sweeper alone.
        let Some(service_name) = reported.service_name else {
            continue;
        };
        if service_name == SMITHD_SERVICE_NAME {
            continue;
        }

        let previous = reported.previous_active_state.as_deref();
        let down_now = is_service_down(&status.active_state);
        let down_before = previous.is_some_and(is_service_down);

        if down_now && !down_before {
            // Deliberately not backdated: this report is the first moment the
            // failure was observed, and it could have happened any time since the
            // previous one. Under-reporting by one ping beats inventing a start.
            sqlx::query!(
                "INSERT INTO device_service_outage (device_id, service_name, started_at)
                 VALUES ($1, $2, NOW())
                 ON CONFLICT DO NOTHING",
                device_id,
                service_name
            )
            .execute(&mut *tx)
            .await?;
        } else if status.active_state == "active" && previous != Some("active") {
            sqlx::query!(
                "UPDATE device_service_outage SET ended_at = NOW()
                 WHERE device_id = $1 AND service_name = $2 AND ended_at IS NULL",
                device_id,
                service_name
            )
            .execute(&mut *tx)
            .await?;
        }
    }
    tx.commit().await?;
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
    let mut command_ids = Vec::new();

    let mut tx = pool.begin().await?;

    let bundle_id = sqlx::query!(
        r#"INSERT INTO command_bundles (user_id) VALUES ($1) RETURNING uuid"#,
        user_id
    )
    .fetch_one(&mut *tx)
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
        .fetch_one(&mut *tx)
        .await?;

        command_ids.push(command_id);
    }

    tx.commit().await?;
    Ok(command_ids)
}

#[cfg(test)]
mod tests {
    use super::map_key_mgmt;

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
}
