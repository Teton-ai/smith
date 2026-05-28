//! Device session manager: keeps a short-lived JWT (minted by smith api's
//! `/auth/session`) in memory and refreshes it before expiry. Outbound HTTP
//! callers ask this actor for `bearer_token()` and get either the JWT (fast
//! path, smith api verifies locally) or the long-lived opaque token from
//! magic.toml as fallback when no JWT is available yet.
//!
//! The JWT is never persisted — on restart the daemon re-mints it from the
//! opaque token. This means a smith-api outage longer than the JWT lifetime
//! degrades us back to opaque-token traffic, which still works.

use crate::magic::MagicHandle;
use crate::shutdown::ShutdownSignals;
use anyhow::{Context, anyhow};
use reqwest::StatusCode;
use serde::Deserialize;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info, warn};

/// Outcome of an attempt to mint a new JWT from the opaque token.
/// Lets callers distinguish "refresh token is dead, give up" from
/// "transient blip, try again later".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshOutcome {
    Refreshed,
    /// `/auth/session` returned 401 — the opaque token is revoked.
    Unauthorized,
    /// Network error, 5xx, parse failure, or no opaque token yet.
    Transient,
}

#[derive(Debug)]
enum RefreshError {
    Unauthorized,
    Other(anyhow::Error),
}

impl std::fmt::Display for RefreshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RefreshError::Unauthorized => write!(f, "opaque token rejected (401)"),
            RefreshError::Other(err) => write!(f, "{err:#}"),
        }
    }
}

/// How long before expiry to start refreshing.
const REFRESH_LEAD_SECS: u64 = 600;
/// Backoff after a failed refresh.
const REFRESH_BACKOFF_SECS: u64 = 60;
/// Default lifetime if smith api doesn't tell us.
const DEFAULT_TTL_SECS: u64 = 3600;

#[derive(Debug, Deserialize)]
struct SessionResponse {
    token: String,
    expires_in: u64,
}

enum SessionMessage {
    GetBearer {
        rpc: oneshot::Sender<Option<String>>,
    },
    ForceRefresh {
        rpc: oneshot::Sender<RefreshOutcome>,
    },
}

struct Session {
    shutdown: ShutdownSignals,
    receiver: mpsc::Receiver<SessionMessage>,
    magic: MagicHandle,
    http: reqwest::Client,
    jwt: Option<String>,
    /// Unix seconds when the current JWT expires.
    expires_at: u64,
}

impl Session {
    fn new(
        shutdown: ShutdownSignals,
        receiver: mpsc::Receiver<SessionMessage>,
        magic: MagicHandle,
    ) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .gzip(true)
            .build()
            .unwrap_or_else(|err| {
                error!("Failed to build session HTTP client, using default: {err:?}");
                reqwest::Client::new()
            });

        Self {
            shutdown,
            receiver,
            magic,
            http,
            jwt: None,
            expires_at: 0,
        }
    }

    fn now_secs() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    fn seconds_until_refresh(&self) -> u64 {
        let now = Self::now_secs();
        if self.expires_at <= now + REFRESH_LEAD_SECS {
            0
        } else {
            self.expires_at - now - REFRESH_LEAD_SECS
        }
    }

    async fn refresh(&mut self) -> std::result::Result<(), RefreshError> {
        let opaque = self.magic.get_token().await.ok_or_else(|| {
            RefreshError::Other(anyhow!("no opaque device token available; cannot mint JWT"))
        })?;
        let server = self.magic.get_server().await;

        let url = build_session_url(&server).map_err(RefreshError::Other)?;

        let response = self
            .http
            .get(url)
            .header("Authorization", format!("Bearer {}", opaque))
            .send()
            .await
            .context("smith api /auth/session request failed")
            .map_err(RefreshError::Other)?;

        let status = response.status();
        if status == StatusCode::UNAUTHORIZED {
            return Err(RefreshError::Unauthorized);
        }
        if !status.is_success() {
            return Err(RefreshError::Other(anyhow!(
                "smith api /auth/session returned {}",
                status
            )));
        }

        let body: SessionResponse = response
            .json()
            .await
            .context("failed to parse /auth/session response")
            .map_err(RefreshError::Other)?;

        let ttl = if body.expires_in == 0 {
            DEFAULT_TTL_SECS
        } else {
            body.expires_in
        };
        self.expires_at = Self::now_secs() + ttl;
        self.jwt = Some(body.token);
        info!("Refreshed device JWT; expires in {}s", ttl);
        Ok(())
    }

    async fn handle_message(&mut self, msg: SessionMessage) {
        match msg {
            SessionMessage::GetBearer { rpc } => {
                // Hand out the JWT if we have an unexpired one; otherwise fall
                // back to the opaque token so callers can still make progress.
                let bearer = if self.jwt.is_some() && self.expires_at > Self::now_secs() {
                    self.jwt.clone()
                } else {
                    self.magic.get_token().await
                };
                let _ = rpc.send(bearer);
            }
            SessionMessage::ForceRefresh { rpc } => {
                let outcome = match self.refresh().await {
                    Ok(()) => RefreshOutcome::Refreshed,
                    Err(RefreshError::Unauthorized) => {
                        warn!("Forced JWT refresh: opaque token rejected (401)");
                        RefreshOutcome::Unauthorized
                    }
                    Err(RefreshError::Other(err)) => {
                        warn!("Forced JWT refresh failed: {err:#}");
                        RefreshOutcome::Transient
                    }
                };
                let _ = rpc.send(outcome);
            }
        }
    }

    async fn run(&mut self) {
        info!("Session manager running");

        // Initial mint: don't loop forever on failure — postman registers the
        // device, and the periodic refresh below will pick up once the opaque
        // token is available.
        if let Err(err) = self.refresh().await {
            debug!("Initial JWT refresh deferred: {err:#}");
        }

        loop {
            let sleep_secs = if self.jwt.is_some() {
                self.seconds_until_refresh().max(1)
            } else {
                REFRESH_BACKOFF_SECS
            };

            tokio::select! {
                Some(msg) = self.receiver.recv() => {
                    self.handle_message(msg).await;
                }
                _ = tokio::time::sleep(Duration::from_secs(sleep_secs)) => {
                    if let Err(err) = self.refresh().await {
                        warn!("JWT refresh failed, will retry: {err:#}");
                        // On failure, expire the cached JWT so we keep
                        // retrying and callers fall back to the opaque token.
                        if self.expires_at <= Self::now_secs() {
                            self.jwt = None;
                        }
                    }
                }
                _ = self.shutdown.token.cancelled() => {
                    break;
                }
            }
        }

        info!("Session task shut down");
    }
}

/// Magic stores the server as e.g. `http://api:8080/smith`. /auth/session lives
/// at the api root, not under /smith, so strip the trailing path segment.
fn build_session_url(server: &str) -> anyhow::Result<String> {
    let trimmed = server.trim_end_matches('/');
    let base = trimmed.strip_suffix("/smith").unwrap_or(trimmed);
    Ok(format!("{}/auth/session", base))
}

#[derive(Clone)]
pub struct SessionHandle {
    sender: mpsc::Sender<SessionMessage>,
}

impl SessionHandle {
    pub fn new(shutdown: ShutdownSignals, magic: MagicHandle) -> Self {
        let (sender, receiver) = mpsc::channel(8);
        let mut actor = Session::new(shutdown, receiver, magic);
        tokio::spawn(async move { actor.run().await });
        Self { sender }
    }

    /// Returns the JWT if a valid one is cached, otherwise the opaque token
    /// from magic. Returns `None` only if the device has no opaque token yet
    /// (i.e. not registered).
    pub async fn bearer_token(&self) -> Option<String> {
        let (rpc, fut) = oneshot::channel();
        if self
            .sender
            .send(SessionMessage::GetBearer { rpc })
            .await
            .is_err()
        {
            return None;
        }
        fut.await.ok().flatten()
    }

    /// Try to mint a fresh JWT from the opaque token. Call this when the API
    /// returns 401 so callers can distinguish a recoverable JWT expiry from a
    /// terminal opaque-token revocation.
    pub async fn force_refresh(&self) -> RefreshOutcome {
        let (rpc, fut) = oneshot::channel();
        if let Err(err) = self.sender.send(SessionMessage::ForceRefresh { rpc }).await {
            error!("Failed to enqueue force-refresh: {err:?}");
            return RefreshOutcome::Transient;
        }
        fut.await.unwrap_or(RefreshOutcome::Transient)
    }
}
