//! Welcome to the documentation center for Agent smith
//!
//! Agent smith is a binary that is tasked with running in our embedded devices
//! its responsible for monitoring the health of the device and reporting it to
//! our backend.
//! The binary is run as a systemd service on the devices.
//!

use smith::control;
use smith::control::Outcome;
use smith::daemon;
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    // setup logging
    tracing_subscriber::fmt::init();

    match control::execute().await {
        Outcome::RunDaemon => {
            daemon::run().await;
            ExitCode::SUCCESS
        }
        Outcome::Success => ExitCode::SUCCESS,
        Outcome::Failure => ExitCode::FAILURE,
    }
}
