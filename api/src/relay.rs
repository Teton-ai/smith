//! Cross-replica session plumbing shared by every device session kind.
//!
//! A device session spans several sockets — a file browse has four (dashboard
//! control, device control, device upload, browser download), a log stream has
//! two — and the api runs multiple replicas with no session stickiness, so they
//! routinely land on different processes. An in-memory session map only works
//! when both ends happen to hit the same replica.
//!
//! Postgres is already the one thing every replica shares, so it carries both
//! the session lifecycle and the relay. `NOTIFY` payloads are capped at 8000
//! bytes and a directory listing exceeds that, so the frame goes in
//! `session_message` and the notification carries only the row id. Consumers
//! `DELETE ... RETURNING`, which makes delivery atomic — exactly one replica
//! gets each frame — and self-cleaning.

use anyhow::Result;
use serde_json::Value;
use sqlx::PgPool;
use sqlx::postgres::PgListener;
use tracing::{error, warn};
use uuid::Uuid;

/// Which feature owns a session. Stored on `stream_session.kind` so one relay,
/// one sweeper and one lifecycle serve all of them.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Files,
    Logs,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Files => "files",
            Kind::Logs => "logs",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    ToDevice,
    ToDashboard,
}

impl Direction {
    fn as_str(self) -> &'static str {
        match self {
            Direction::ToDevice => "to_device",
            Direction::ToDashboard => "to_dashboard",
        }
    }

    /// Postgres identifiers are lowercased unless quoted, and a UUID contains
    /// hyphens, so the channel name is quoted at both LISTEN and NOTIFY.
    fn channel(self, session_id: &Uuid) -> String {
        format!("session_{}_{}", session_id.simple(), self.as_str())
    }
}

pub struct SessionRow {
    pub device_id: i32,
    pub user_id: Option<i32>,
}

pub async fn create_session(
    pool: &PgPool,
    session_id: &Uuid,
    kind: Kind,
    device_id: i32,
    user_id: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO stream_session (id, kind, device_id, user_id) VALUES ($1, $2, $3, $4)",
        session_id,
        kind.as_str(),
        device_id,
        user_id
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Look up an open session of the given kind. Returns `None` once it has been
/// closed, so a device dialling back into a finished session is rejected rather
/// than served, and `None` across kinds so a log session id cannot be used to
/// attach to the file browser.
pub async fn lookup_open(
    pool: &PgPool,
    session_id: &Uuid,
    kind: Kind,
) -> Result<Option<SessionRow>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT device_id, user_id FROM stream_session
        WHERE id = $1 AND kind = $2 AND closed_at IS NULL
        "#,
        session_id,
        kind.as_str()
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|row| SessionRow {
        device_id: row.device_id,
        user_id: row.user_id,
    }))
}

/// Look up a session regardless of whether it is still open. Used when writing
/// audit rows after the fact, where attribution matters but liveness doesn't.
pub async fn lookup_any(
    pool: &PgPool,
    session_id: &Uuid,
) -> Result<Option<SessionRow>, sqlx::Error> {
    let row = sqlx::query!(
        "SELECT device_id, user_id FROM stream_session WHERE id = $1",
        session_id
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|row| SessionRow {
        device_id: row.device_id,
        user_id: row.user_id,
    }))
}

pub async fn mark_device_connected(pool: &PgPool, session_id: &Uuid) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE stream_session SET device_connected = true WHERE id = $1",
        session_id
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn close_session(pool: &PgPool, session_id: &Uuid) {
    sqlx::query!(
        "UPDATE stream_session SET closed_at = now() WHERE id = $1 AND closed_at IS NULL",
        session_id
    )
    .execute(pool)
    .await
    .inspect_err(|e| error!("Failed to close session {session_id}: {e}"))
    .ok();
}

/// Abandoned sessions and their relay backlog. Sessions are normally closed by
/// their own handler; this catches the ones whose replica died mid-session.
pub async fn sweep_stale_sessions(pool: &PgPool) -> Result<u64, sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE stream_session
        SET closed_at = now()
        WHERE closed_at IS NULL AND created_at < now() - interval '1 hour'
        "#
    )
    .execute(pool)
    .await?;

    let result =
        sqlx::query!("DELETE FROM session_message WHERE created_at < now() - interval '1 hour'")
            .execute(pool)
            .await?;

    Ok(result.rows_affected())
}

/// Persist a frame and wake whichever replica owns the far end.
pub async fn publish(
    pool: &PgPool,
    session_id: &Uuid,
    direction: Direction,
    payload: &Value,
) -> Result<()> {
    let row = sqlx::query!(
        r#"
        INSERT INTO session_message (session_id, direction, payload)
        VALUES ($1, $2, $3)
        RETURNING id
        "#,
        session_id,
        direction.as_str(),
        payload
    )
    .fetch_one(pool)
    .await?;

    // NOTIFY is transactional: it fires at commit of the implicit transaction
    // above, so a listener can never be woken for a row it cannot yet read.
    sqlx::query("SELECT pg_notify($1, $2)")
        .bind(direction.channel(session_id))
        .bind(row.id.to_string())
        .execute(pool)
        .await?;

    Ok(())
}

/// A subscription to one direction of one session.
pub struct Subscription {
    listener: PgListener,
    pool: PgPool,
}

impl Subscription {
    /// Takes the database URL rather than the pool on purpose.
    /// `PgListener::connect_with` would `acquire()` a pooled connection and hold
    /// it for the whole session — up to 30 minutes. Two subscriptions per
    /// session against a 100-connection pool means ~50 concurrent sessions would
    /// starve every other request the api serves. A standalone connection keeps
    /// that pressure off the request path.
    pub async fn open(
        pool: &PgPool,
        database_url: &str,
        session_id: &Uuid,
        direction: Direction,
    ) -> Result<Self> {
        let mut listener = PgListener::connect(database_url).await?;
        listener.listen(&direction.channel(session_id)).await?;
        Ok(Self {
            listener,
            pool: pool.clone(),
        })
    }

    /// Next frame for this subscription, or `None` once the connection drops.
    ///
    /// A notification whose row is already gone is skipped rather than
    /// returned: another replica consumed it, which is normal and not an error.
    pub async fn next(&mut self) -> Option<Value> {
        loop {
            let notification = match self.listener.recv().await {
                Ok(notification) => notification,
                Err(e) => {
                    error!("Session listener failed: {e}");
                    return None;
                }
            };

            let Ok(message_id) = notification.payload().parse::<i64>() else {
                warn!(
                    "Ignoring malformed session notification: {}",
                    notification.payload()
                );
                continue;
            };

            let row = sqlx::query!(
                "DELETE FROM session_message WHERE id = $1 RETURNING payload",
                message_id
            )
            .fetch_optional(&self.pool)
            .await;

            match row {
                Ok(Some(row)) => return Some(row.payload),
                Ok(None) => continue,
                Err(e) => {
                    error!("Failed to consume session message: {e}");
                    return None;
                }
            }
        }
    }
}

/// Drain frames queued for a session before the far end started listening.
///
/// The device dials in some seconds after the dashboard socket opens, so
/// anything published in that window has already fired its NOTIFY into the
/// void. Without this the first operation of every session would hang.
pub async fn drain_pending(
    pool: &PgPool,
    session_id: &Uuid,
    direction: Direction,
) -> Result<Vec<Value>> {
    let rows = sqlx::query!(
        r#"
        DELETE FROM session_message
        WHERE id IN (
            SELECT id FROM session_message
            WHERE session_id = $1 AND direction = $2
            ORDER BY id
        )
        RETURNING payload
        "#,
        session_id,
        direction.as_str()
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|row| row.payload).collect())
}
