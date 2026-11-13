use axum::http::StatusCode;
use axum::response::IntoResponse;
use std::borrow::Cow;

#[derive(Debug)]
pub enum ApiError {
    /// 400 Bad Request
    BadRequest(Cow<'static, str>),
    /// 403 Forbidden, with the reason shown to the user
    Forbidden(Cow<'static, str>),
    /// 404 Not Found
    NotFound,
    /// 500 Internal Server Error
    #[allow(dead_code)]
    InternalServerError(anyhow::Error),
    /// Any other status, with no message
    Status(StatusCode),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::BadRequest(msg) | ApiError::Forbidden(msg) => f.write_str(msg),
            ApiError::NotFound => f.write_str("not found"),
            ApiError::InternalServerError(err) => write!(f, "{err}"),
            ApiError::Status(status) => write!(f, "{status}"),
        }
    }
}

impl std::error::Error for ApiError {}

impl ApiError {
    /// Logs failures that are ours. A refusal is the API working as intended, and
    /// its reason already went to the caller.
    pub fn log_if_unexpected(&self, context: impl std::fmt::Display) {
        if matches!(self, Self::InternalServerError(_)) {
            tracing::error!("{context}: {self:?}");
        }
    }
}

impl From<StatusCode> for ApiError {
    fn from(status: StatusCode) -> Self {
        Self::Status(status)
    }
}

impl ApiError {
    pub fn bad_request<Msg: Into<Cow<'static, str>>>(msg: Msg) -> Self {
        Self::BadRequest(msg.into())
    }
}

impl From<sqlx::error::Error> for ApiError {
    fn from(e: sqlx::error::Error) -> Self {
        match e {
            sqlx::Error::RowNotFound => Self::NotFound,
            _ => Self::InternalServerError(e.into()),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        match self {
            ApiError::BadRequest(cow) => (StatusCode::BAD_REQUEST, cow).into_response(),
            ApiError::Forbidden(cow) => (StatusCode::FORBIDDEN, cow).into_response(),
            ApiError::NotFound => StatusCode::NOT_FOUND.into_response(),
            ApiError::InternalServerError(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            ApiError::Status(status) => status.into_response(),
        }
    }
}
