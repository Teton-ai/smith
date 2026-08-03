//! Daemon↔API end-to-end scenarios. Each test is self-sufficient (own seed +
//! device check, own commands/releases) so they tolerate any order and dirty
//! databases; run them serially since they share the single device container:
//!
//!     cargo test -p smith-e2e -- --ignored --test-threads=1

use anyhow::{Context as _, Result, ensure};
use smith_e2e::{
    Ctx, UPGRADE_TIMEOUT, enqueue, ensure_device_online, seed_minimum, wait_for_api,
    wait_for_response, wait_until,
};
use sqlx::Row;

/// Stage 3 content-addressed writes (`network_content_lock_key` /
/// `network_find_by_content`) do not exist in a released API image, so this
/// test only makes sense against the HEAD API. Runtime-checked (not the
/// `network_content_lock_key` migration name) so it works regardless of which
/// CI job invoked it: absent the function, it is the version-skew
/// (released-api) job, and the test is a no-op rather than a failure.
async fn has_stage3_content_addressing(ctx: &Ctx) -> Result<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pg_proc WHERE proname = 'network_content_lock_key')",
    )
    .fetch_one(&ctx.db)
    .await
    .context("checking for network_content_lock_key")
}

async fn setup() -> Result<(Ctx, i32)> {
    let ctx = Ctx::connect().await?;
    wait_for_api(&ctx).await?;
    seed_minimum(&ctx).await?;
    let device_id = ensure_device_online(&ctx).await?;
    Ok((ctx, device_id))
}

#[tokio::test]
#[ignore = "requires running compose stack; use make test.e2e"]
async fn device_registers_and_comes_online() -> Result<()> {
    let (ctx, device_id) = setup().await?;

    let row = sqlx::query(
        "SELECT approved, token, last_ping IS NOT NULL AS has_ping
         FROM device WHERE id = $1",
    )
    .bind(device_id)
    .fetch_one(&ctx.db)
    .await
    .context("reading device row")?;

    ensure!(row.get::<bool, _>("approved"), "device should be approved");
    let token = row
        .get::<Option<String>, _>("token")
        .context("device should hold a token")?;
    ensure!(!token.is_empty(), "device token should not be empty");
    ensure!(row.get::<bool, _>("has_ping"), "device should have pinged");
    Ok(())
}

#[tokio::test]
#[ignore = "requires running compose stack; use make test.e2e"]
async fn freeform_command_round_trip() -> Result<()> {
    let (ctx, device_id) = setup().await?;

    let marker = format!("e2e-{}", uuid::Uuid::new_v4());
    let command_id = enqueue(
        &ctx,
        device_id,
        &format!(r#"{{"FreeForm":{{"cmd":"echo {marker}"}}}}"#),
    )
    .await?;

    let (response, status) = wait_for_response(&ctx, command_id).await?;
    ensure!(status == 0, "expected status 0, got {status}: {response}");
    let stdout = response["FreeForm"]["stdout"]
        .as_str()
        .with_context(|| format!("expected FreeForm response, got {response}"))?;
    ensure!(
        stdout.contains(&marker),
        "stdout should contain {marker}, got {stdout:?}"
    );

    let fetched = sqlx::query("SELECT fetched FROM command_queue WHERE id = $1")
        .bind(command_id)
        .fetch_one(&ctx.db)
        .await
        .context("reading command_queue row")?
        .get::<bool, _>("fetched");
    ensure!(fetched, "command should be marked fetched");
    Ok(())
}

#[tokio::test]
#[ignore = "requires running compose stack; use make test.e2e"]
async fn ping_pong_round_trip() -> Result<()> {
    let (ctx, device_id) = setup().await?;

    let command_id = enqueue(&ctx, device_id, r#""Ping""#).await?;
    let (response, status) = wait_for_response(&ctx, command_id).await?;

    ensure!(status == 0, "expected status 0, got {status}: {response}");
    ensure!(
        response == serde_json::json!("Pong"),
        "expected \"Pong\", got {response}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires running compose stack; use make test.e2e"]
async fn empty_release_upgrade_reports_release_id() -> Result<()> {
    let (ctx, device_id) = setup().await?;

    // A release with no packages exercises the full upgrade loop (manifest
    // fetch, updater run, release_id report-back) without needing S3.
    let version = format!("0.0.0-e2e-{}", uuid::Uuid::new_v4());
    let release_id = sqlx::query(
        "INSERT INTO release (distribution_id, version, draft)
         SELECT id, $1, false FROM distribution WHERE name = 'e2e'
         RETURNING id",
    )
    .bind(&version)
    .fetch_one(&ctx.db)
    .await
    .context("creating empty release")?
    .get::<i32, _>("id");

    sqlx::query("UPDATE device SET target_release_id = $1 WHERE id = $2")
        .bind(release_id)
        .bind(device_id)
        .execute(&ctx.db)
        .await
        .context("targeting release")?;

    // The Upgrade command makes the daemon upgrade immediately instead of
    // waiting for the updater's 60s check tick.
    let command_id = enqueue(&ctx, device_id, r#""Upgrade""#).await?;
    let (response, status) = wait_for_response(&ctx, command_id).await?;
    ensure!(
        status == 0,
        "Upgrade command failed with status {status}: {response}"
    );

    wait_until(
        &format!("device to report release_id {release_id}"),
        UPGRADE_TIMEOUT,
        || async {
            let row = sqlx::query("SELECT id FROM device WHERE id = $1 AND release_id = $2")
                .bind(device_id)
                .bind(release_id)
                .fetch_optional(&ctx.db)
                .await
                .context("polling device release_id")?;
            Ok(row.map(|_| ()))
        },
    )
    .await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires running compose stack; use make test.e2e"]
async fn concurrent_identical_network_posts_converge_to_one_row() -> Result<()> {
    let ctx = Ctx::connect().await?;
    // The API applies the migrations on startup, so the pg_proc probe below only
    // means "released image" once the API is actually up. Without this wait, a
    // HEAD run that starts before the API has migrated would take the skip branch
    // and pass green without ever running the race.
    wait_for_api(&ctx).await?;

    if !has_stage3_content_addressing(&ctx).await? {
        println!(
            "skipping concurrent_identical_network_posts_converge_to_one_row: \
             network_content_lock_key not present (released-api version-skew job)"
        );
        return Ok(());
    }

    // A DB-level equivalent of `POST /networks` racing itself: exercises the
    // actual lock -> match -> insert mechanism shared by create_network and
    // ReportNMProfiles, without needing an Auth0 token for the HTTP route.
    let ssid = format!("e2e-race-{}", uuid::Uuid::new_v4());
    let is_hidden = false;
    let security_type = "wpa-psk";
    let credentials = serde_json::json!({ "psk": "e2e-race-password" });

    const WRITERS: usize = 8;
    let mut writers = Vec::with_capacity(WRITERS);
    for _ in 0..WRITERS {
        let pool = ctx.db.clone();
        let ssid = ssid.clone();
        let credentials = credentials.clone();
        writers.push(tokio::spawn(async move {
            let mut tx = pool.begin().await?;

            sqlx::query("SELECT pg_advisory_xact_lock(network_content_lock_key($1, $2, $3))")
                .bind(&ssid)
                .bind(is_hidden)
                .bind(&credentials)
                .execute(&mut *tx)
                .await?;

            let existing_id: Option<i32> =
                sqlx::query_scalar("SELECT network_find_by_content($1, $2, $3, $4, 'wifi')")
                    .bind(&ssid)
                    .bind(is_hidden)
                    .bind(&credentials)
                    .bind(security_type)
                    .fetch_one(&mut *tx)
                    .await?;

            let id: i32 = match existing_id {
                Some(id) => id,
                None => {
                    sqlx::query_scalar(
                        "INSERT INTO network
                             (ssid, password, name, network_type, is_network_hidden,
                              security_type, credentials)
                         VALUES ($1, $2, $1, 'wifi', $3, $4, $5)
                         RETURNING id",
                    )
                    .bind(&ssid)
                    .bind("e2e-race-password")
                    .bind(is_hidden)
                    .bind(security_type)
                    .bind(&credentials)
                    .fetch_one(&mut *tx)
                    .await?
                }
            };

            tx.commit().await?;
            anyhow::Ok(id)
        }));
    }

    // Wrapped so a writer panic or a query failure still reaches the cleanup
    // below, not just a failing assertion: any early `?` return here would
    // otherwise leak this row into the database.
    let outcome: Result<(i64, bool, Vec<i32>)> = async {
        let mut ids = Vec::with_capacity(WRITERS);
        for writer in writers {
            ids.push(writer.await.context("writer task panicked")??);
        }

        let row_count: i64 = sqlx::query_scalar("SELECT count(*) FROM network WHERE ssid = $1")
            .bind(&ssid)
            .fetch_one(&ctx.db)
            .await
            .context("counting rows for race ssid")?;
        let all_converged = ids.windows(2).all(|pair| pair[0] == pair[1]);

        Ok((row_count, all_converged, ids))
    }
    .await;

    sqlx::query("DELETE FROM network WHERE ssid = $1")
        .bind(&ssid)
        .execute(&ctx.db)
        .await
        .context("cleaning up race ssid")?;

    let (row_count, all_converged, ids) = outcome?;

    ensure!(
        row_count == 1,
        "expected exactly one network row for {ssid}, got {row_count}"
    );
    ensure!(
        all_converged,
        "all writers should have converged on the same id, got {ids:?}"
    );

    Ok(())
}

/// Regression guard for the write/read asymmetry that content addressing makes
/// fatal: `ReportNMProfiles` looks a row up by `credentials->>'psk'`, so it must
/// also *store* that psk. It briefly derived the stored envelope from
/// `security_type` instead, which dropped the psk whenever the type was unknown
/// (a pre-Stage-2 daemon sending no `key_mgmt`, or an nmcli value outside
/// `map_key_mgmt` such as `ieee8021x`). The row was then invisible to the very
/// key that had created it, so every 20s report cycle forked another duplicate.
///
/// Drives the real handler over `POST /smith/home` using the device's own token,
/// rather than re-implementing the SQL, so a regression in the Rust side is
/// caught too. Reporting overwrites this device's `device_configured_network`
/// snapshot and `current_network_id`; the daemon restores both on its next
/// report, and no other test asserts on them.
#[tokio::test]
#[ignore = "requires running compose stack; use make test.e2e"]
async fn repeated_untyped_profile_report_does_not_duplicate() -> Result<()> {
    let (ctx, device_id) = setup().await?;

    if !has_stage3_content_addressing(&ctx).await? {
        println!(
            "skipping repeated_untyped_profile_report_does_not_duplicate: \
             network_content_lock_key not present (released-api version-skew job)"
        );
        return Ok(());
    }

    let token: String = sqlx::query("SELECT token FROM device WHERE id = $1")
        .bind(device_id)
        .fetch_one(&ctx.db)
        .await
        .context("reading device token")?
        .get::<Option<String>, _>("token")
        .context("device should hold a token")?;

    let ssid = format!("e2e-untyped-{}", uuid::Uuid::new_v4());
    // No `key_mgmt`: exactly what a pre-Stage-2 daemon sends, and what an
    // unrecognised nmcli key-mgmt value degrades to.
    let body = serde_json::json!({
        "timestamp": { "secs": 0, "nanos": 0 },
        "release_id": null,
        "service_statuses": [],
        "responses": [{
            // Negative ids are stored with a NULL command_id, so no queued
            // command has to exist for this synthetic report.
            "id": -9,
            "status": 0,
            "command": { "ReportNMProfiles": { "profiles": [{
                "name": ssid,
                "ssid": ssid,
                "password": "e2e-untyped-password",
                "is_active": false,
            }]}},
        }],
    });

    // Wrapped so a failed report or query still reaches the cleanup below, not
    // just a failing assertion: any early `?`/in-loop `ensure!` return here
    // would otherwise leak a row (from a prior successful attempt) into the
    // database.
    let outcome: Result<(i64, Option<String>)> = async {
        for attempt in 1..=2 {
            let response = ctx
                .http
                .post(format!("{}/smith/home", ctx.api_url))
                .bearer_auth(&token)
                .json(&body)
                .send()
                .await
                .with_context(|| format!("posting profile report (attempt {attempt})"))?;
            ensure!(
                response.status().is_success(),
                "report {attempt} failed with {}",
                response.status()
            );
        }

        let row_count: i64 = sqlx::query_scalar("SELECT count(*) FROM network WHERE ssid = $1")
            .bind(&ssid)
            .fetch_one(&ctx.db)
            .await
            .context("counting rows for untyped ssid")?;

        // The stored envelope must carry the psk the match projects, or the
        // next report forks a duplicate even though this one did not.
        let psk: Option<String> =
            sqlx::query_scalar("SELECT credentials->>'psk' FROM network WHERE ssid = $1")
                .bind(&ssid)
                .fetch_one(&ctx.db)
                .await
                .context("reading stored psk")?;

        Ok((row_count, psk))
    }
    .await;

    // Cleanup runs unconditionally. Configured-network and current_network_id
    // are cleared first since both reference network.id.
    sqlx::query("DELETE FROM device_configured_network WHERE network_id IN (SELECT id FROM network WHERE ssid = $1)")
        .bind(&ssid)
        .execute(&ctx.db)
        .await
        .context("cleaning up configured-network rows")?;
    sqlx::query("UPDATE device SET current_network_id = NULL WHERE current_network_id IN (SELECT id FROM network WHERE ssid = $1)")
        .bind(&ssid)
        .execute(&ctx.db)
        .await
        .context("clearing current_network_id")?;
    sqlx::query("DELETE FROM network WHERE ssid = $1")
        .bind(&ssid)
        .execute(&ctx.db)
        .await
        .context("cleaning up untyped ssid")?;

    let (row_count, psk) = outcome?;

    ensure!(
        row_count == 1,
        "two identical untyped reports should yield one network row, got {row_count}"
    );
    ensure!(
        psk.as_deref() == Some("e2e-untyped-password"),
        "stored credentials must carry the psk used to find the row, got {psk:?}"
    );

    Ok(())
}

/// Regression guard for the EAP blind spot: with no shared psk, every field the
/// pre-identity match compared was equal for two EAP rows on one SSID. Distinct
/// identities must not match each other, and an unknown identity must still reach
/// an identified row, since POST /networks and old smithd never send one.
#[tokio::test]
#[ignore = "requires running compose stack; use make test.e2e"]
async fn distinct_eap_identities_on_one_ssid_do_not_match() -> Result<()> {
    let ctx = Ctx::connect().await?;
    wait_for_api(&ctx).await?;

    if !has_stage3_content_addressing(&ctx).await? {
        println!(
            "skipping distinct_eap_identities_on_one_ssid_do_not_match: \
             network_content_lock_key not present (released-api version-skew job)"
        );
        return Ok(());
    }

    let ssid = format!("e2e-eap-{}", uuid::Uuid::new_v4());
    // No psk: the psk clause matches trivially, leaving identity as the only field
    // that can tell the two rows apart.
    let credentials = serde_json::json!({ "eap": "peap", "phase2_auth": "mschapv2" });
    let identity_a = serde_json::json!({ "username": "e2e-box-a" });
    let identity_b = serde_json::json!({ "username": "e2e-box-b" });
    let identity_c = serde_json::json!({ "username": "e2e-box-c" });

    let insert = |identity: Option<serde_json::Value>| {
        let (ssid, credentials) = (ssid.clone(), credentials.clone());
        let pool = ctx.db.clone();
        async move {
            sqlx::query_scalar::<_, i32>(
                "INSERT INTO network
                     (ssid, name, network_type, is_network_hidden,
                      security_type, credentials, identity)
                 VALUES ($1, $1, 'wifi', false, 'wpa-eap', $2, $3)
                 RETURNING id",
            )
            .bind(&ssid)
            .bind(&credentials)
            .bind(identity)
            .fetch_one(&pool)
            .await
        }
    };

    let find = |identity: Option<serde_json::Value>| {
        let (ssid, credentials) = (ssid.clone(), credentials.clone());
        let pool = ctx.db.clone();
        async move {
            sqlx::query_scalar::<_, Option<i32>>(
                "SELECT network_find_by_content($1, false, $2, 'wpa-eap', 'wifi', $3)",
            )
            .bind(&ssid)
            .bind(&credentials)
            .bind(identity)
            .fetch_one(&pool)
            .await
        }
    };

    struct Outcome {
        a_id: i32,
        matched_a: Option<i32>,
        matched_b_before_insert: Option<i32>,
        matched_b_after_insert: Option<i32>,
        matched_unknown: Option<i32>,
        matched_healable: Option<i32>,
        b_id: i32,
        null_ident_id: i32,
    }

    // Wrapped so a failed insert or match still reaches the cleanup below rather
    // than leaking the rows inserted up to that point.
    let outcome: Result<Outcome> = async {
        let a_id = insert(Some(identity_a.clone()))
            .await
            .context("inserting EAP row A")?;

        let matched_a = find(Some(identity_a.clone()))
            .await
            .context("matching identity A")?;

        // The bug: with only A present, identity B used to be handed A's id.
        let matched_b_before_insert = find(Some(identity_b.clone()))
            .await
            .context("matching identity B against row A alone")?;

        let b_id = insert(Some(identity_b.clone()))
            .await
            .context("inserting EAP row B")?;

        let matched_b_after_insert = find(Some(identity_b.clone()))
            .await
            .context("matching identity B with both rows present")?;

        let matched_unknown = find(None).await.context("matching without an identity")?;

        // Inserted last on purpose: an identity-NULL row is relaxed against every
        // p_identity, so it would win the ordering tiebreak and change every
        // assertion above. Identity C matches neither A nor B exactly, leaving the
        // NULL row as the only candidate, which is the healing direction.
        let null_ident_id = insert(None).await.context("inserting identity-NULL row")?;

        let matched_healable = find(Some(identity_c.clone()))
            .await
            .context("matching a third identity against the identity-NULL row")?;

        Ok(Outcome {
            a_id,
            matched_a,
            matched_b_before_insert,
            matched_b_after_insert,
            matched_unknown,
            matched_healable,
            b_id,
            null_ident_id,
        })
    }
    .await;

    sqlx::query("DELETE FROM network WHERE ssid = $1")
        .bind(&ssid)
        .execute(&ctx.db)
        .await
        .context("cleaning up eap ssid")?;

    let Outcome {
        a_id,
        matched_a,
        matched_b_before_insert,
        matched_b_after_insert,
        matched_unknown,
        matched_healable,
        b_id,
        null_ident_id,
    } = outcome?;

    ensure!(
        matched_a == Some(a_id),
        "identity A must match its own row {a_id}, got {matched_a:?}"
    );
    ensure!(
        matched_b_before_insert.is_none(),
        "identity B must not match row {a_id}, which holds identity A; got \
         {matched_b_before_insert:?}"
    );
    ensure!(
        matched_b_after_insert == Some(b_id),
        "identity B must match its own row {b_id}, got {matched_b_after_insert:?}"
    );
    ensure!(
        matched_unknown == Some(b_id),
        "a writer with no identity must stay relaxed and reach the newest row {b_id} \
         (id DESC tiebreak among equally-relaxed matches), got {matched_unknown:?}"
    );
    ensure!(
        matched_healable == Some(null_ident_id),
        "an identified writer must reach the identity-NULL row {null_ident_id} to heal it, \
         and must not match rows {a_id} or {b_id} which hold other identities; got \
         {matched_healable:?}"
    );

    Ok(())
}

/// Regression guard for the healing half of F4 and for the match ordering that
/// makes it safe. A typed writer must reach an existing NULL-`security_type` row
/// (rather than inserting a second one) and fill the type in, but when an exact
/// twin also exists it must prefer that twin instead of "healing" the NULL row
/// into a duplicate of it.
#[tokio::test]
#[ignore = "requires running compose stack; use make test.e2e"]
async fn typed_match_prefers_exact_row_over_null_row() -> Result<()> {
    let ctx = Ctx::connect().await?;
    wait_for_api(&ctx).await?;

    if !has_stage3_content_addressing(&ctx).await? {
        println!(
            "skipping typed_match_prefers_exact_row_over_null_row: \
             network_content_lock_key not present (released-api version-skew job)"
        );
        return Ok(());
    }

    let ssid = format!("e2e-order-{}", uuid::Uuid::new_v4());
    let credentials = serde_json::json!({ "psk": "e2e-order-password" });

    let insert = |security_type: Option<&'static str>| {
        let (ssid, credentials) = (ssid.clone(), credentials.clone());
        let pool = ctx.db.clone();
        async move {
            sqlx::query_scalar::<_, i32>(
                "INSERT INTO network
                     (ssid, password, name, network_type, is_network_hidden,
                      security_type, credentials)
                 VALUES ($1, $2, $1, 'wifi', false, $3, $4)
                 RETURNING id",
            )
            .bind(&ssid)
            .bind("e2e-order-password")
            .bind(security_type)
            .bind(&credentials)
            .fetch_one(&pool)
            .await
        }
    };

    /// Every value the final assertions need, produced by the fallible section
    /// below. Named fields instead of a positional tuple since there are seven
    /// of them.
    struct Outcome {
        matched_exact: Option<i32>,
        matched_against_newer_null: Option<i32>,
        matched_untyped: Option<i32>,
        matched_ethernet: Option<i32>,
        null_id: i32,
        typed_id: i32,
        newer_null_id: i32,
    }

    // Wrapped so a failed insert or match query still reaches the cleanup
    // below, not just a failing assertion: any early `?` return here would
    // otherwise leak the rows already inserted at that point.
    let outcome: Result<Outcome> = async {
        // NULL row first, typed twin second, so `id DESC` alone would still
        // pick the typed one. Insert them the other way round too, below.
        let null_id = insert(None).await.context("inserting NULL-security row")?;
        let typed_id = insert(Some("wpa-psk"))
            .await
            .context("inserting typed row")?;

        let matched_exact: Option<i32> =
            sqlx::query_scalar("SELECT network_find_by_content($1, false, $2, 'wpa-psk', 'wifi')")
                .bind(&ssid)
                .bind(&credentials)
                .fetch_one(&ctx.db)
                .await
                .context("matching with a typed security")?;

        // With the typed row now older than the NULL row, `id DESC` on its own
        // would return the NULL row; only the exact-match preference gets this
        // right.
        let newer_null_id = insert(None)
            .await
            .context("inserting second NULL-security row")?;
        let matched_against_newer_null: Option<i32> =
            sqlx::query_scalar("SELECT network_find_by_content($1, false, $2, 'wpa-psk', 'wifi')")
                .bind(&ssid)
                .bind(&credentials)
                .fetch_one(&ctx.db)
                .await
                .context("matching with a typed security against a newer NULL row")?;

        // The relaxation is bidirectional: an untyped writer still reaches a row.
        let matched_untyped: Option<i32> =
            sqlx::query_scalar("SELECT network_find_by_content($1, false, $2, NULL, 'wifi')")
                .bind(&ssid)
                .bind(&credentials)
                .fetch_one(&ctx.db)
                .await
                .context("matching with an unknown security")?;

        // network_type is part of the identity: the same content under a
        // different type is a different network, not a match.
        let matched_ethernet: Option<i32> = sqlx::query_scalar(
            "SELECT network_find_by_content($1, false, $2, 'wpa-psk', 'ethernet')",
        )
        .bind(&ssid)
        .bind(&credentials)
        .fetch_one(&ctx.db)
        .await
        .context("matching across network types")?;

        Ok(Outcome {
            matched_exact,
            matched_against_newer_null,
            matched_untyped,
            matched_ethernet,
            null_id,
            typed_id,
            newer_null_id,
        })
    }
    .await;

    sqlx::query("DELETE FROM network WHERE ssid = $1")
        .bind(&ssid)
        .execute(&ctx.db)
        .await
        .context("cleaning up order ssid")?;

    let Outcome {
        matched_exact,
        matched_against_newer_null,
        matched_untyped,
        matched_ethernet,
        null_id,
        typed_id,
        newer_null_id,
    } = outcome?;

    ensure!(
        matched_exact == Some(typed_id),
        "a typed writer should prefer the exact row {typed_id} over the NULL row {null_id}, got {matched_exact:?}"
    );
    ensure!(
        matched_against_newer_null == Some(typed_id),
        "exact match {typed_id} must win over the newer NULL row {newer_null_id}, got {matched_against_newer_null:?}"
    );
    ensure!(
        matched_untyped == Some(newer_null_id),
        "an untyped writer should match the newest NULL-security row {newer_null_id} (id DESC \
         tiebreak among equally-relaxed matches), got {matched_untyped:?}"
    );
    ensure!(
        matched_ethernet.is_none(),
        "an ethernet writer must not match a wifi row, got {matched_ethernet:?}"
    );

    Ok(())
}
