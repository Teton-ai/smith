/// The network-reference-ledger holder identity resolved from the caller's M2M
/// token, or `None` for a caller with no resolved holder (a dashboard/CLI user
/// token, or an M2M token whose `sub` isn't in `Config::known_holders`).
///
/// Always inserted into request extensions by `middlewares::authentication::check`
/// (never left absent), so ledger handlers can extract `Extension<Holder>`
/// unconditionally and turn `None` into a `403` themselves, rather than hitting
/// axum's default `500` for a missing extension.
#[derive(Clone, Debug)]
pub struct Holder(pub Option<String>);
