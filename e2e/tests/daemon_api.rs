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

/// Regression guard for `network_find_by_content`'s match ordering: a typed
/// writer must prefer an exact `security_type` twin over a same-SSID row with a
/// different type, even a newer one (`id DESC` alone would pick that instead).
///
/// Used to also cover NULL-`security_type` "healing", dropped because
/// `network_security_type_wifi_check` now makes that state unreachable.
#[tokio::test]
#[ignore = "requires running compose stack; use make test.e2e"]
async fn typed_match_prefers_exact_row_over_mismatched_type() -> Result<()> {
    let ctx = Ctx::connect().await?;
    wait_for_api(&ctx).await?;

    if !has_stage3_content_addressing(&ctx).await? {
        println!(
            "skipping typed_match_prefers_exact_row_over_mismatched_type: \
             network_content_lock_key not present (released-api version-skew job)"
        );
        return Ok(());
    }

    let ssid = format!("e2e-order-{}", uuid::Uuid::new_v4());
    let credentials = serde_json::json!({ "psk": "e2e-order-password" });

    let insert = |security_type: &'static str| {
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
    /// below. Named fields instead of a positional tuple since there are five
    /// of them.
    struct Outcome {
        matched_exact: Option<i32>,
        matched_against_newer_mismatch: Option<i32>,
        matched_ethernet: Option<i32>,
        exact_id: i32,
        mismatch_id: i32,
    }

    // Wrapped so a failed insert or match query still reaches the cleanup
    // below, not just a failing assertion: any early `?` return here would
    // otherwise leak the rows already inserted at that point.
    let outcome: Result<Outcome> = async {
        // Exact-type row first, mismatch second, so `id DESC` alone would pick
        // the mismatch.
        let exact_id = insert("wpa-psk")
            .await
            .context("inserting exact-type row")?;
        let mismatch_id = insert("open")
            .await
            .context("inserting mismatched-type row")?;

        let matched_exact: Option<i32> =
            sqlx::query_scalar("SELECT network_find_by_content($1, false, $2, 'wpa-psk', 'wifi')")
                .bind(&ssid)
                .bind(&credentials)
                .fetch_one(&ctx.db)
                .await
                .context("matching with a typed security")?;

        // A third distinct type: repeating "open" would collide with the
        // first mismatch under network_ident_uq.
        let _newer_mismatch_id = insert("wpa-eap")
            .await
            .context("inserting second mismatched-type row")?;
        let matched_against_newer_mismatch: Option<i32> =
            sqlx::query_scalar("SELECT network_find_by_content($1, false, $2, 'wpa-psk', 'wifi')")
                .bind(&ssid)
                .bind(&credentials)
                .fetch_one(&ctx.db)
                .await
                .context("matching with a typed security against a newer mismatch")?;

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
            matched_against_newer_mismatch,
            matched_ethernet,
            exact_id,
            mismatch_id,
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
        matched_against_newer_mismatch,
        matched_ethernet,
        exact_id,
        mismatch_id,
    } = outcome?;

    ensure!(
        matched_exact == Some(exact_id),
        "a typed writer should prefer the exact row {exact_id} over the mismatched row {mismatch_id}, got {matched_exact:?}"
    );
    ensure!(
        matched_against_newer_mismatch == Some(exact_id),
        "exact match {exact_id} must win over a newer mismatched row, got {matched_against_newer_mismatch:?}"
    );
    ensure!(
        matched_ethernet.is_none(),
        "an ethernet writer must not match a wifi row, got {matched_ethernet:?}"
    );

    Ok(())
}

// --- Reference ledger -----------------------------------------------------
//
// The ledger endpoints (`POST/DELETE /networks/{id}/references`,
// `POST /networks/references/reconcile`) sit behind the Auth0 `check()`
// middleware, which this harness has no way to mint a token for (see this
// file's own top comment: every scenario here drives Postgres directly
// instead, exactly as the dashboard's Auth0-gated routes already do). So,
// like `concurrent_identical_network_posts_converge_to_one_row` above, these
// tests replicate the handlers' SQL directly rather than going over HTTP.
// The pure reconcile-diff algorithm itself (add/remove set computation) is
// covered separately by unit tests in `api/src/network/ledger.rs`; what's
// worth an e2e test is the DB-only behavior a unit test can't reach: locking,
// the `ON DELETE RESTRICT` interaction, and transaction atomicity.

/// `network_has_internal_reference` only exists once the reference ledger's
/// application code has landed; its absence means a released-api
/// version-skew run, not a bug.
async fn has_reference_ledger(ctx: &Ctx) -> Result<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pg_proc WHERE proname = 'network_has_internal_reference')",
    )
    .fetch_one(&ctx.db)
    .await
    .context("checking for network_has_internal_reference")
}

/// Minimal wifi network row for ledger tests: `security_type` must be set
/// (a wifi-scoped CHECK requires it), everything else can be a placeholder.
async fn insert_test_network(ctx: &Ctx, ssid: &str) -> Result<i32> {
    sqlx::query_scalar(
        "INSERT INTO network (network_type, is_network_hidden, ssid, name, security_type)
         VALUES ('wifi', false, $1, $1, 'open')
         RETURNING id",
    )
    .bind(ssid)
    .fetch_one(&ctx.db)
    .await
    .context("inserting test network")
}

async fn insert_reference(
    ctx: &Ctx,
    holder: &str,
    external_key: &str,
    network_id: i32,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO network_reference (holder, external_key, network_id)
         VALUES ($1, $2, $3)
         ON CONFLICT (holder, external_key, network_id) DO NOTHING",
    )
    .bind(holder)
    .bind(external_key)
    .bind(network_id)
    .execute(&ctx.db)
    .await
    .context("inserting test network_reference")?;
    Ok(())
}

async fn network_reference_count(ctx: &Ctx, network_id: i32) -> Result<i64> {
    sqlx::query_scalar("SELECT count(*) FROM network_reference WHERE network_id = $1")
        .bind(network_id)
        .fetch_one(&ctx.db)
        .await
        .context("counting network_reference rows")
}

async fn network_exists(ctx: &Ctx, network_id: i32) -> Result<bool> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM network WHERE id = $1)")
        .bind(network_id)
        .fetch_one(&ctx.db)
        .await
        .context("checking network existence")
}

/// Mirrors `acquire_reference`'s insert: `ON CONFLICT DO NOTHING` on the full
/// `(holder, external_key, network_id)` triple.
#[tokio::test]
#[ignore = "requires running compose stack; use make test.e2e"]
async fn acquire_reference_is_idempotent() -> Result<()> {
    let ctx = Ctx::connect().await?;
    wait_for_api(&ctx).await?;
    if !has_reference_ledger(&ctx).await? {
        println!("skipping acquire_reference_is_idempotent: ledger not present (version-skew job)");
        return Ok(());
    }

    let ssid = format!("e2e-ledger-acquire-{}", uuid::Uuid::new_v4());
    let outcome: Result<i64> = async {
        let network_id = insert_test_network(&ctx, &ssid).await?;
        insert_reference(&ctx, "app_api", &ssid, network_id).await?;
        insert_reference(&ctx, "app_api", &ssid, network_id).await?; // idempotent repeat
        network_reference_count(&ctx, network_id).await
    }
    .await;

    // network_reference's ON DELETE RESTRICT means the network row can't go
    // first: this test leaves a real, committed reference behind (unlike
    // e.g. release_last_reference_collects_unreferenced_network, which clears
    // its own reference as part of what it's testing).
    sqlx::query("DELETE FROM network_reference WHERE external_key = $1")
        .bind(&ssid)
        .execute(&ctx.db)
        .await
        .context("cleaning up acquire-idempotency reference")?;
    sqlx::query("DELETE FROM network WHERE ssid = $1")
        .bind(&ssid)
        .execute(&ctx.db)
        .await
        .context("cleaning up acquire-idempotency network")?;

    let count = outcome?;
    ensure!(
        count == 1,
        "expected exactly one reference row, got {count}"
    );
    Ok(())
}

/// Mirrors `release_reference`: delete the ledger row, then `collect_network`'s
/// check (hand-copied - keep in sync). With zero ledger rows and zero
/// internal FK references, the network row itself must be deleted as a
/// side effect.
#[tokio::test]
#[ignore = "requires running compose stack; use make test.e2e"]
async fn release_last_reference_collects_unreferenced_network() -> Result<()> {
    let ctx = Ctx::connect().await?;
    wait_for_api(&ctx).await?;
    if !has_reference_ledger(&ctx).await? {
        println!(
            "skipping release_last_reference_collects_unreferenced_network: \
             ledger not present (version-skew job)"
        );
        return Ok(());
    }

    let ssid = format!("e2e-ledger-collect-{}", uuid::Uuid::new_v4());
    let outcome: Result<bool> = async {
        let network_id = insert_test_network(&ctx, &ssid).await?;
        insert_reference(&ctx, "app_api", &ssid, network_id).await?;

        let mut tx = ctx.db.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(i64::from(network_id))
            .execute(&mut *tx)
            .await?;
        let rows_affected = sqlx::query(
            "DELETE FROM network_reference WHERE holder = $1 AND external_key = $2 AND network_id = $3",
        )
        .bind("app_api")
        .bind(&ssid)
        .bind(network_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if rows_affected > 0 {
            let has_ledger_ref: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM network_reference WHERE network_id = $1)",
            )
            .bind(network_id)
            .fetch_one(&mut *tx)
            .await?;
            let has_internal_ref: bool =
                sqlx::query_scalar("SELECT network_has_internal_reference($1)")
                    .bind(network_id)
                    .fetch_one(&mut *tx)
                    .await?;
            if !has_ledger_ref && !has_internal_ref {
                sqlx::query("DELETE FROM network WHERE id = $1")
                    .bind(network_id)
                    .execute(&mut *tx)
                    .await?;
            }
        }
        tx.commit().await?;

        network_exists(&ctx, network_id).await
    }
    .await;

    // In the expected (success) path the reference is already gone by the
    // time the transaction above commits, but if the test body errors before
    // that commit, the reference `insert_reference` created up front is still
    // real and committed - same reasoning as acquire_reference_is_idempotent.
    sqlx::query("DELETE FROM network_reference WHERE external_key = $1")
        .bind(&ssid)
        .execute(&ctx.db)
        .await
        .context("cleaning up collect-on-release reference")?;
    sqlx::query("DELETE FROM network WHERE ssid = $1")
        .bind(&ssid)
        .execute(&ctx.db)
        .await
        .context("cleaning up collect-on-release network")?;

    ensure!(
        !outcome?,
        "network with zero ledger and zero internal references should have been collected"
    );
    Ok(())
}

/// A release whose `(holder, external_key, network_id)` matches no row must
/// not attempt collection at all - otherwise any authenticated holder could
/// use a network_id/external_key it never registered to garbage-collect a
/// network it has no relationship to, merely because that network happened
/// to be at zero references already.
#[tokio::test]
#[ignore = "requires running compose stack; use make test.e2e"]
async fn release_of_an_unheld_reference_does_not_collect() -> Result<()> {
    let ctx = Ctx::connect().await?;
    wait_for_api(&ctx).await?;
    if !has_reference_ledger(&ctx).await? {
        println!(
            "skipping release_of_an_unheld_reference_does_not_collect: \
             ledger not present (version-skew job)"
        );
        return Ok(());
    }

    let ssid = format!("e2e-ledger-unheld-release-{}", uuid::Uuid::new_v4());
    let outcome: Result<bool> = async {
        // No insert_reference call: this network already has zero ledger and
        // zero internal references from the moment it's created - exactly
        // the state a buggy/unauthorized release must not be able to exploit.
        let network_id = insert_test_network(&ctx, &ssid).await?;

        let mut tx = ctx.db.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(i64::from(network_id))
            .execute(&mut *tx)
            .await?;
        let rows_affected = sqlx::query(
            "DELETE FROM network_reference WHERE holder = $1 AND external_key = $2 AND network_id = $3",
        )
        .bind("app_api")
        .bind("never-registered-key")
        .bind(network_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if rows_affected > 0 {
            let has_ledger_ref: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM network_reference WHERE network_id = $1)",
            )
            .bind(network_id)
            .fetch_one(&mut *tx)
            .await?;
            let has_internal_ref: bool =
                sqlx::query_scalar("SELECT network_has_internal_reference($1)")
                    .bind(network_id)
                    .fetch_one(&mut *tx)
                    .await?;
            if !has_ledger_ref && !has_internal_ref {
                sqlx::query("DELETE FROM network WHERE id = $1")
                    .bind(network_id)
                    .execute(&mut *tx)
                    .await?;
            }
        }
        tx.commit().await?;

        network_exists(&ctx, network_id).await
    }
    .await;

    sqlx::query("DELETE FROM network WHERE ssid = $1")
        .bind(&ssid)
        .execute(&ctx.db)
        .await
        .context("cleaning up unheld-release network")?;

    ensure!(
        outcome?,
        "a release matching no hold must not collect an unrelated network"
    );
    Ok(())
}

/// Same mechanism as above, but a `device_configured_network` row still points
/// at the network: `collect_network` must leave it alone even though its
/// ledger reference count just hit zero.
#[tokio::test]
#[ignore = "requires running compose stack; use make test.e2e"]
async fn release_does_not_collect_while_internal_reference_remains() -> Result<()> {
    let (ctx, device_id) = setup().await?;
    if !has_reference_ledger(&ctx).await? {
        println!(
            "skipping release_does_not_collect_while_internal_reference_remains: \
             ledger not present (version-skew job)"
        );
        return Ok(());
    }

    let ssid = format!("e2e-ledger-internal-{}", uuid::Uuid::new_v4());
    let outcome: Result<bool> = async {
        let network_id = insert_test_network(&ctx, &ssid).await?;
        insert_reference(&ctx, "app_api", &ssid, network_id).await?;
        sqlx::query(
            "INSERT INTO device_configured_network (device_id, network_id, profile_name)
             VALUES ($1, $2, $3)",
        )
        .bind(device_id)
        .bind(network_id)
        .bind(&ssid)
        .execute(&ctx.db)
        .await?;

        let mut tx = ctx.db.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(i64::from(network_id))
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "DELETE FROM network_reference WHERE holder = $1 AND external_key = $2 AND network_id = $3",
        )
        .bind("app_api")
        .bind(&ssid)
        .bind(network_id)
        .execute(&mut *tx)
        .await?;
        let has_internal_ref: bool =
            sqlx::query_scalar("SELECT network_has_internal_reference($1)")
                .bind(network_id)
                .fetch_one(&mut *tx)
                .await?;
        if !has_internal_ref {
            sqlx::query("DELETE FROM network WHERE id = $1")
                .bind(network_id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;

        network_exists(&ctx, network_id).await
    }
    .await;

    sqlx::query("DELETE FROM device_configured_network WHERE network_id IN (SELECT id FROM network WHERE ssid = $1)")
        .bind(&ssid)
        .execute(&ctx.db)
        .await
        .context("cleaning up configured-network row")?;
    // Same reasoning as the sibling collect-on-release test above: a
    // committed reference can outlive an errored test body.
    sqlx::query("DELETE FROM network_reference WHERE external_key = $1")
        .bind(&ssid)
        .execute(&ctx.db)
        .await
        .context("cleaning up internal-reference reference")?;
    sqlx::query("DELETE FROM network WHERE ssid = $1")
        .bind(&ssid)
        .execute(&ctx.db)
        .await
        .context("cleaning up internal-reference network")?;

    ensure!(
        outcome?,
        "network with a surviving device_configured_network row must not be collected"
    );
    Ok(())
}

/// Mirrors the transaction-level guarantee `reconcile_references` relies on:
/// an insert referencing a nonexistent `network_id` must roll back every
/// insert the same transaction already staged, not just skip the bad one.
/// `reconcile_references` itself now pre-validates `to_add` network_ids and
/// returns `400` before this FK path is ever reached in practice; this test
/// exercises the underlying DB guarantee directly as a backstop.
#[tokio::test]
#[ignore = "requires running compose stack; use make test.e2e"]
async fn reconcile_rolls_back_entirely_on_invalid_element() -> Result<()> {
    let ctx = Ctx::connect().await?;
    wait_for_api(&ctx).await?;
    if !has_reference_ledger(&ctx).await? {
        println!(
            "skipping reconcile_rolls_back_entirely_on_invalid_element: \
             ledger not present (version-skew job)"
        );
        return Ok(());
    }

    let ssid = format!("e2e-ledger-reconcile-atomic-{}", uuid::Uuid::new_v4());
    let holder = format!("e2e-holder-{}", uuid::Uuid::new_v4());
    let outcome: Result<i64> = async {
        let network_id = insert_test_network(&ctx, &ssid).await?;
        // Same nonexistent id every run would collide across parallel test
        // runs sharing the same DB; derive one from a value that cannot exist
        // (negative ids are never issued by the IDENTITY sequence).
        let bogus_network_id = -1;

        let mut tx = ctx.db.begin().await?;
        let good_insert = sqlx::query(
            "INSERT INTO network_reference (holder, external_key, network_id) VALUES ($1, $2, $3)",
        )
        .bind(&holder)
        .bind(&ssid)
        .bind(network_id)
        .execute(&mut *tx)
        .await;
        ensure!(
            good_insert.is_ok(),
            "the valid insert should not fail on its own"
        );

        let bad_insert = sqlx::query(
            "INSERT INTO network_reference (holder, external_key, network_id) VALUES ($1, $2, $3)",
        )
        .bind(&holder)
        .bind(&ssid)
        .bind(bogus_network_id)
        .execute(&mut *tx)
        .await;
        ensure!(
            bad_insert.is_err(),
            "inserting a reference for a nonexistent network_id should violate the FK"
        );
        // Explicit for clarity; a dropped, uncommitted transaction rolls back
        // the same way.
        tx.rollback().await?;

        network_reference_count(&ctx, network_id).await
    }
    .await;

    sqlx::query("DELETE FROM network WHERE ssid = $1")
        .bind(&ssid)
        .execute(&ctx.db)
        .await
        .context("cleaning up reconcile-atomicity network")?;

    let count = outcome?;
    ensure!(
        count == 0,
        "the valid insert must not survive the rollback triggered by the invalid one, got {count} rows"
    );
    Ok(())
}

/// `network_reference`'s `ON DELETE RESTRICT` FK means the existing public
/// `DELETE /networks/{id}` can no longer silently orphan a held network now
/// that the ledger exists: it must fail instead.
#[tokio::test]
#[ignore = "requires running compose stack; use make test.e2e"]
async fn hard_delete_fails_on_a_held_network() -> Result<()> {
    let ctx = Ctx::connect().await?;
    wait_for_api(&ctx).await?;
    if !has_reference_ledger(&ctx).await? {
        println!(
            "skipping hard_delete_fails_on_a_held_network: ledger not present (version-skew job)"
        );
        return Ok(());
    }

    let ssid = format!("e2e-ledger-restrict-{}", uuid::Uuid::new_v4());
    let outcome: Result<bool> = async {
        let network_id = insert_test_network(&ctx, &ssid).await?;
        insert_reference(&ctx, "app_api", &ssid, network_id).await?;

        let delete_result = sqlx::query("DELETE FROM network WHERE id = $1")
            .bind(network_id)
            .execute(&ctx.db)
            .await;
        Ok(delete_result.is_err())
    }
    .await;

    sqlx::query("DELETE FROM network_reference WHERE external_key = $1")
        .bind(&ssid)
        .execute(&ctx.db)
        .await
        .context("cleaning up restrict-test reference")?;
    sqlx::query("DELETE FROM network WHERE ssid = $1")
        .bind(&ssid)
        .execute(&ctx.db)
        .await
        .context("cleaning up restrict-test network")?;

    ensure!(
        outcome?,
        "DELETE FROM network on a held network should fail under ON DELETE RESTRICT"
    );
    Ok(())
}

/// `reconcile_references` locks every network_id it touches in ascending
/// order specifically to avoid deadlocking against another overlapping
/// reconcile/acquire/release call - a claim that only means something under
/// real concurrency. N tasks lock the *same two* network ids in ascending
/// order, racing on which gets there first; drop the ordering and this
/// reliably deadlocks, keep it and every task completes.
///
/// Simulates the invariant rather than calling `reconcile_references`'s own
/// `.sort_unstable()` (same reason as every other test in this section): a
/// regression that drops that call from `ledger.rs` would slip past this.
#[tokio::test]
#[ignore = "requires running compose stack; use make test.e2e"]
async fn ascending_lock_order_avoids_deadlock_under_concurrency() -> Result<()> {
    let ctx = Ctx::connect().await?;
    wait_for_api(&ctx).await?;
    if !has_reference_ledger(&ctx).await? {
        println!(
            "skipping ascending_lock_order_avoids_deadlock_under_concurrency: \
             ledger not present (version-skew job)"
        );
        return Ok(());
    }

    let ssid_a = format!("e2e-ledger-lock-a-{}", uuid::Uuid::new_v4());
    let ssid_b = format!("e2e-ledger-lock-b-{}", uuid::Uuid::new_v4());
    let outcome: Result<()> = async {
        let mut id_a = insert_test_network(&ctx, &ssid_a).await?;
        let mut id_b = insert_test_network(&ctx, &ssid_b).await?;
        if id_a > id_b {
            std::mem::swap(&mut id_a, &mut id_b);
        }

        const WRITERS: usize = 8;
        let mut writers = Vec::with_capacity(WRITERS);
        for _ in 0..WRITERS {
            let pool = ctx.db.clone();
            writers.push(tokio::spawn(async move {
                let mut tx = pool.begin().await?;
                // Ascending order, every task, every time - the invariant
                // `reconcile_references` relies on.
                sqlx::query("SELECT pg_advisory_xact_lock($1)")
                    .bind(i64::from(id_a))
                    .execute(&mut *tx)
                    .await?;
                sqlx::query("SELECT pg_advisory_xact_lock($1)")
                    .bind(i64::from(id_b))
                    .execute(&mut *tx)
                    .await?;
                tx.commit().await?;
                anyhow::Ok(())
            }));
        }

        for writer in writers {
            writer.await.context("lock-order writer task panicked")??;
        }
        Ok(())
    }
    .await;

    sqlx::query("DELETE FROM network WHERE ssid IN ($1, $2)")
        .bind(&ssid_a)
        .bind(&ssid_b)
        .execute(&ctx.db)
        .await
        .context("cleaning up lock-order networks")?;

    outcome.context("concurrent ascending-order lockers should all complete without deadlock")?;
    Ok(())
}
