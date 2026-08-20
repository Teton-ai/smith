//! The network reference ledger: acquire/release/reconcile for external
//! holders (App API today) plus `collect_network`, the private collection
//! check that deletes a network once nothing references it anymore.
//!
//! `holder` is never read from a request body anywhere in this module: it is
//! always the caller's ledger identity resolved server-side from their M2M
//! token by `middlewares::authentication::check` (see `crate::holder::Holder`),
//! so a caller can't release or reconcile another holder's references by
//! spoofing the field. A caller with no resolved holder is rejected with `403`
//! before any DB work.

use crate::State;
use crate::holder::Holder;
use axum::extract::{Extension, Json, Path};
use axum::http::StatusCode;
use serde::Deserialize;
use sqlx::PgConnection;
use std::collections::{BTreeSet, HashSet};
use utoipa::ToSchema;

use super::route::{NETWORKS_TAG, internal_error};

#[derive(Debug, Deserialize, ToSchema)]
pub struct ReferenceRequest {
    pub external_key: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ReconcileKey {
    pub external_key: String,
    pub network_id: i32,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ReconcileRequest {
    pub keys: Vec<ReconcileKey>,
}

async fn lock_network(tx: &mut PgConnection, network_id: i32) -> Result<(), sqlx::Error> {
    sqlx::query!("SELECT pg_advisory_xact_lock($1)", i64::from(network_id))
        .execute(tx)
        .await?;
    Ok(())
}

/// Serializes every `network_reference` mutation for one holder against every
/// other mutation for that holder. Without it, reconcile's read of what a
/// holder currently holds could race a concurrent acquire/create adding a row
/// on a `network_id` nothing has locked via `lock_network` yet - this makes
/// that read-then-act atomic per holder. Salted (`1`) to avoid colliding with
/// `network_content_lock_key`'s unsalted (`0`) hash in the same
/// `pg_advisory_xact_lock(bigint)` keyspace.
pub(crate) async fn lock_holder(tx: &mut PgConnection, holder: &str) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "SELECT pg_advisory_xact_lock(pg_catalog.hashtextextended($1, 1))",
        holder
    )
    .execute(tx)
    .await?;
    Ok(())
}

/// The one construction of a `network_reference` insert; shared verbatim by
/// `acquire_reference` and `create_network`'s `reference` field so the two
/// insert paths cannot silently diverge. Idempotent: re-registering an
/// already-held `(holder, external_key, network_id)` triple is a no-op, not an
/// error. Deliberately does not call `collect_network` - a row this just added
/// a hold to is never a collection candidate in the same breath.
pub(crate) async fn insert_reference(
    tx: &mut PgConnection,
    holder: &str,
    external_key: &str,
    network_id: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO network_reference (holder, external_key, network_id)
        VALUES ($1, $2, $3)
        ON CONFLICT (holder, external_key, network_id) DO NOTHING
        "#,
        holder,
        external_key,
        network_id,
    )
    .execute(tx)
    .await?;
    Ok(())
}

/// Given a `network_id` whose advisory lock (`pg_advisory_xact_lock`, taken by
/// the caller for the lifetime of `tx`) is already held, deletes the network
/// row if it has zero ledger references and zero internal FK references.
/// `network_reference`'s `ON DELETE RESTRICT` FK is why this order matters:
/// both counts are verified zero first, so the delete below can never be the
/// thing that would have violated it.
///
/// `e2e/tests/daemon_api.rs` hand-copies this check (can't call it directly -
/// see that file's header); keep the two in sync.
pub(crate) async fn collect_network(
    tx: &mut PgConnection,
    network_id: i32,
) -> Result<bool, sqlx::Error> {
    let has_ledger_reference = sqlx::query_scalar!(
        r#"SELECT EXISTS(SELECT 1 FROM network_reference WHERE network_id = $1) AS "exists!""#,
        network_id
    )
    .fetch_one(&mut *tx)
    .await?;
    if has_ledger_reference {
        return Ok(false);
    }

    let has_internal_reference = sqlx::query_scalar!(
        r#"SELECT network_has_internal_reference($1) AS "exists!""#,
        network_id
    )
    .fetch_one(&mut *tx)
    .await?;
    if has_internal_reference {
        return Ok(false);
    }

    sqlx::query!("DELETE FROM network WHERE id = $1", network_id)
        .execute(tx)
        .await?;
    Ok(true)
}

#[utoipa::path(
    post,
    path = "/networks/{network_id}/references",
    params(("network_id" = i32, Path)),
    request_body = ReferenceRequest,
    responses(
        (status = 204, description = "Reference held (idempotent: a no-op if already held)"),
        (status = 403, description = "Caller has no resolved ledger holder identity"),
        (status = 404, description = "Network does not exist"),
        (status = 500, description = "Failed to acquire reference", body = String),
    ),
    security(("auth_token" = [])),
    tag = NETWORKS_TAG
)]
pub async fn acquire_reference(
    Extension(state): Extension<State>,
    Extension(Holder(holder)): Extension<Holder>,
    Path(network_id): Path<i32>,
    Json(body): Json<ReferenceRequest>,
) -> Result<StatusCode, StatusCode> {
    let holder = holder.ok_or(StatusCode::FORBIDDEN)?;

    let mut tx = state.pg_pool.begin().await.map_err(internal_error(
        "Failed to begin acquire_reference transaction",
    ))?;

    lock_holder(&mut tx, &holder)
        .await
        .map_err(internal_error("Failed to take holder lock"))?;
    lock_network(&mut tx, network_id)
        .await
        .map_err(internal_error("Failed to take network lock"))?;

    // Existence check kept deliberately separate from the insert below rather
    // than inferred from an FK-violation error code: network_reference's only
    // FK today is to network(id), so that inference happens to be safe, but a
    // future FK on this table would silently turn an unrelated violation into
    // a wrong 404.
    let exists: bool = sqlx::query_scalar!(
        r#"SELECT EXISTS(SELECT 1 FROM network WHERE id = $1) AS "exists!""#,
        network_id
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(internal_error("Failed to check network existence"))?;
    if !exists {
        return Err(StatusCode::NOT_FOUND);
    }

    insert_reference(&mut tx, &holder, &body.external_key, network_id)
        .await
        .map_err(internal_error("Failed to insert network reference"))?;

    tx.commit().await.map_err(internal_error(
        "Failed to commit acquire_reference transaction",
    ))?;

    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    delete,
    path = "/networks/{network_id}/references",
    params(("network_id" = i32, Path)),
    request_body = ReferenceRequest,
    responses(
        (status = 204, description = "Reference released; always returned, regardless of whether a row was deleted or collection happened"),
        (status = 403, description = "Caller has no resolved ledger holder identity"),
        (status = 500, description = "Failed to release reference", body = String),
    ),
    security(("auth_token" = [])),
    tag = NETWORKS_TAG
)]
pub async fn release_reference(
    Extension(state): Extension<State>,
    Extension(Holder(holder)): Extension<Holder>,
    Path(network_id): Path<i32>,
    Json(body): Json<ReferenceRequest>,
) -> Result<StatusCode, StatusCode> {
    let holder = holder.ok_or(StatusCode::FORBIDDEN)?;

    let mut tx = state.pg_pool.begin().await.map_err(internal_error(
        "Failed to begin release_reference transaction",
    ))?;

    lock_holder(&mut tx, &holder)
        .await
        .map_err(internal_error("Failed to take holder lock"))?;
    lock_network(&mut tx, network_id)
        .await
        .map_err(internal_error("Failed to take network lock"))?;

    sqlx::query!(
        "DELETE FROM network_reference WHERE holder = $1 AND external_key = $2 AND network_id = $3",
        holder,
        body.external_key,
        network_id,
    )
    .execute(&mut *tx)
    .await
    .map_err(internal_error("Failed to delete network reference"))?;

    // The caller does not decide and does not inspect internal FK state - 204
    // either way, whether or not this actually collected the row.
    collect_network(&mut tx, network_id)
        .await
        .map_err(internal_error("Failed to attempt network collection"))?;

    tx.commit().await.map_err(internal_error(
        "Failed to commit release_reference transaction",
    ))?;

    Ok(StatusCode::NO_CONTENT)
}

async fn fetch_holder_references(
    tx: &mut PgConnection,
    holder: &str,
) -> Result<Vec<(String, i32)>, sqlx::Error> {
    let rows = sqlx::query!(
        "SELECT external_key, network_id FROM network_reference WHERE holder = $1",
        holder
    )
    .fetch_all(tx)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| (r.external_key, r.network_id))
        .collect())
}

/// Pure set diff, factored out of `reconcile_references` so it is unit-testable
/// without a database.
struct ReconcileDiff {
    to_add: Vec<(String, i32)>,
    to_remove: Vec<(String, i32)>,
}

impl ReconcileDiff {
    fn compute(current: &HashSet<(String, i32)>, desired: &HashSet<(String, i32)>) -> Self {
        ReconcileDiff {
            to_add: desired.difference(current).cloned().collect(),
            to_remove: current.difference(desired).cloned().collect(),
        }
    }

    /// Every `network_id` that lost a hold this call - the only rows collection
    /// needs to be attempted on, since a row nothing was removed from cannot
    /// have just dropped to zero references.
    fn collection_candidates(&self) -> BTreeSet<i32> {
        self.to_remove.iter().map(|(_, id)| *id).collect()
    }
}

#[utoipa::path(
    post,
    path = "/networks/references/reconcile",
    request_body = ReconcileRequest,
    responses(
        (status = 204, description = "This holder's ledger rows now exactly match the pushed key set"),
        (status = 403, description = "Caller has no resolved ledger holder identity"),
        (status = 500, description = "Failed to reconcile references", body = String),
    ),
    security(("auth_token" = [])),
    tag = NETWORKS_TAG
)]
pub async fn reconcile_references(
    Extension(state): Extension<State>,
    Extension(Holder(holder)): Extension<Holder>,
    Json(body): Json<ReconcileRequest>,
) -> Result<StatusCode, StatusCode> {
    let holder = holder.ok_or(StatusCode::FORBIDDEN)?;

    let mut tx = state.pg_pool.begin().await.map_err(internal_error(
        "Failed to begin reconcile_references transaction",
    ))?;

    // Taken before the read below, not after: that's what makes the read a
    // complete, stable view of this holder's rows (see lock_holder's doc).
    lock_holder(&mut tx, &holder)
        .await
        .map_err(internal_error("Failed to take holder lock"))?;

    let current: HashSet<(String, i32)> = fetch_holder_references(&mut tx, &holder)
        .await
        .map_err(internal_error("Failed to read current references"))?
        .into_iter()
        .collect();

    let mut lock_ids: Vec<i32> = body
        .keys
        .iter()
        .map(|k| k.network_id)
        .chain(current.iter().map(|(_, id)| *id))
        .collect();
    lock_ids.sort_unstable();
    lock_ids.dedup();

    for network_id in &lock_ids {
        lock_network(&mut tx, *network_id)
            .await
            .map_err(internal_error("Failed to take network lock"))?;
    }

    let desired: HashSet<(String, i32)> = body
        .keys
        .iter()
        .map(|k| (k.external_key.clone(), k.network_id))
        .collect();

    let diff = ReconcileDiff::compute(&current, &desired);

    for (external_key, network_id) in &diff.to_add {
        insert_reference(&mut tx, &holder, external_key, *network_id)
            .await
            .map_err(internal_error("Failed to insert network reference"))?;
    }

    for (external_key, network_id) in &diff.to_remove {
        sqlx::query!(
            "DELETE FROM network_reference WHERE holder = $1 AND external_key = $2 AND network_id = $3",
            holder,
            external_key,
            network_id,
        )
        .execute(&mut *tx)
        .await
        .map_err(internal_error("Failed to delete network reference"))?;
    }

    for network_id in diff.collection_candidates() {
        collect_network(&mut tx, network_id)
            .await
            .map_err(internal_error("Failed to attempt network collection"))?;
    }

    tx.commit().await.map_err(internal_error(
        "Failed to commit reconcile_references transaction",
    ))?;

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::ReconcileDiff;
    use std::collections::HashSet;

    fn set(pairs: &[(&str, i32)]) -> HashSet<(String, i32)> {
        pairs.iter().map(|(k, id)| (k.to_string(), *id)).collect()
    }

    #[test]
    fn adds_missing_and_removes_extra() {
        let current = set(&[("a", 1), ("b", 2)]);
        let desired = set(&[("b", 2), ("c", 3)]);

        let diff = ReconcileDiff::compute(&current, &desired);

        assert_eq!(diff.to_add, vec![("c".to_string(), 3)]);
        assert_eq!(diff.to_remove, vec![("a".to_string(), 1)]);
        assert_eq!(
            diff.collection_candidates().into_iter().collect::<Vec<_>>(),
            vec![1]
        );
    }

    #[test]
    fn empty_desired_removes_everything() {
        let current = set(&[("a", 1), ("b", 2)]);
        let desired = HashSet::new();

        let diff = ReconcileDiff::compute(&current, &desired);

        assert!(diff.to_add.is_empty());
        let mut removed = diff.to_remove.clone();
        removed.sort();
        assert_eq!(removed, vec![("a".to_string(), 1), ("b".to_string(), 2)]);
    }

    #[test]
    fn identical_sets_produce_no_diff() {
        let current = set(&[("a", 1)]);
        let desired = set(&[("a", 1)]);

        let diff = ReconcileDiff::compute(&current, &desired);

        assert!(diff.to_add.is_empty());
        assert!(diff.to_remove.is_empty());
        assert!(diff.collection_candidates().is_empty());
    }

    #[test]
    fn same_external_key_different_network_id_is_not_a_match() {
        // The ledger PK is the full (holder, external_key, network_id) triple:
        // one holder can reference several network_ids under the same
        // external_key, so these must diff as add+remove, not as "unchanged".
        let current = set(&[("dept-1", 10)]);
        let desired = set(&[("dept-1", 20)]);

        let diff = ReconcileDiff::compute(&current, &desired);

        assert_eq!(diff.to_add, vec![("dept-1".to_string(), 20)]);
        assert_eq!(diff.to_remove, vec![("dept-1".to_string(), 10)]);
    }
}
