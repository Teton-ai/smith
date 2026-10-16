//! Reads the device's network and system settings and reports them to the api
//! (`POST /smith/settings`). Phase 1 only reads: nothing here writes to the
//! system.

pub mod read;
