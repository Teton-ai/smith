mod actor;
mod handler;

pub const PENDING_SMITH_RELEASE_FILE: &str = "pending-smith-release";

pub use handler::Handler as UpdaterHandle;
