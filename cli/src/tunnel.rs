use anyhow::Result;
use russh::keys::*;
use russh::*;
use std::convert::TryFrom;
use std::path::Path;
use std::time::Duration;
use std::{env, sync::Arc};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::ToSocketAddrs;

struct Client {}

// More SSH event handlers
// can be defined in this trait
// In this example, we're only using Channel, so these aren't needed.
impl client::Handler for Client {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

/// This struct is a convenience wrapper
/// around a russh client
/// that handles the input/output event loop
pub struct Session {
    session: client::Handle<Client>,
}

impl Session {
    pub async fn connect<P: AsRef<Path>, A: ToSocketAddrs>(
        key_path: P,
        user: impl Into<String>,
        openssh_cert_path: Option<P>,
        addrs: A,
    ) -> Result<Self> {
        let key_pair = load_secret_key(key_path, None)?;

        // load ssh certificate
        let mut openssh_cert = None;
        if openssh_cert_path.is_some() {
            openssh_cert = Some(load_openssh_certificate(openssh_cert_path.unwrap())?);
        }

        let config = client::Config {
            inactivity_timeout: Some(Duration::from_secs(5)),
            ..<_>::default()
        };

        let config = Arc::new(config);
        let sh = Client {};

        let mut session = client::connect(config, addrs, sh).await?;

        // use publickey authentication, with or without certificate
        if openssh_cert.is_none() {
            let auth_res = session
                .authenticate_publickey(
                    user,
                    PrivateKeyWithHashAlg::new(
                        Arc::new(key_pair),
                        session.best_supported_rsa_hash().await?.flatten(),
                    ),
                )
                .await?;

            if !auth_res.success() {
                anyhow::bail!("Authentication (with publickey) failed");
            }
        } else {
            let auth_res = session
                .authenticate_openssh_cert(user, Arc::new(key_pair), openssh_cert.unwrap())
                .await?;

            if !auth_res.success() {
                anyhow::bail!("Authentication (with publickey+cert) failed");
            }
        }

        Ok(Self { session })
    }

    pub async fn call(&mut self, command: &str) -> Result<u32> {
        let mut channel = self.session.channel_open_session().await?;

        // This example doesn't terminal resizing after the connection is established
        let (w, h) = termion::terminal_size()?;

        // Request an interactive PTY from the server
        channel
            .request_pty(
                false,
                &env::var("TERM").unwrap_or("xterm".into()),
                w as u32,
                h as u32,
                0,
                0,
                &[], // ideally you want to pass the actual terminal modes here
            )
            .await?;
        channel.exec(true, command).await?;

        let code;
        let mut stdin = tokio_fd::AsyncFd::try_from(0)?;
        let mut stdout = tokio_fd::AsyncFd::try_from(1)?;
        let mut buf = vec![0; 1024];
        let mut stdin_closed = false;

        loop {
            // Handle one of the possible events:
            tokio::select! {
                // There's terminal input available from the user
                r = stdin.read(&mut buf), if !stdin_closed => {
                    match r {
                        Ok(0) => {
                            stdin_closed = true;
                            channel.eof().await?;
                        },
                        // Send it to the server
                        Ok(n) => channel.data(&buf[..n]).await?,
                        Err(e) => return Err(e.into()),
                    };
                },
                // There's an event available on the session channel
                Some(msg) = channel.wait() => {
                    match msg {
                        // Write data to the terminal
                        ChannelMsg::Data { ref data } => {
                            stdout.write_all(data).await?;
                            stdout.flush().await?;
                        }
                        // The command has returned an exit code
                        ChannelMsg::ExitStatus { exit_status } => {
                            code = exit_status;
                            if !stdin_closed {
                                channel.eof().await?;
                            }
                            break;
                        }
                        _ => {}
                    }
                },
            }
        }
        Ok(code)
    }

    pub async fn close(&mut self) -> Result<()> {
        self.session
            .disconnect(Disconnect::ByApplication, "", "English")
            .await?;
        Ok(())
    }
}
