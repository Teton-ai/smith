use crate::error::ApiError;
use crate::user::CurrentUser;

pub fn cannot_ship(user: &CurrentUser) -> Option<&'static str> {
    (!user.has_permission("devices", "write")).then_some("Your role does not allow deployments.")
}

pub fn ensure_can_ship(user: &CurrentUser) -> Result<(), ApiError> {
    match cannot_ship(user) {
        Some(reason) => Err(ApiError::Forbidden(reason.into())),
        None => Ok(()),
    }
}
