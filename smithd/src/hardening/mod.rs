//! Security and reliability baseline smithd enforces on every boot. Not a
//! setting: nothing remote can turn these off. Each rule is idempotent and
//! a failure only logs, so a broken rule never stops the daemon from starting.

mod console;
mod ssh;

use tracing::error;

pub async fn run() {
    // self-heals if an os update or someone flips it back
    if let Err(err) = ssh::disable_ssh_password_auth().await {
        error!("Failed to disable SSH password auth: {err:#}");
    }

    // TODO(step 4): v2 only, gate before this ships
    if let Err(err) = console::close_console_logins().await {
        error!("Failed to close console logins: {err:#}");
    }
}
