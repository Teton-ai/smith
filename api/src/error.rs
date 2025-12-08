use axum::http::StatusCode;
use axum::response::IntoResponse;
use std::borrow::Cow;

#[derive(Debug)]
pub enum ApiError {
    /// 400 Bad Request
    BadRequest(Cow<'static, str>),
    /// 404 Not Found
    NotFound,
    /// 500 Internal Server Error
    #[allow(dead_code)]
    InternalServerError(anyhow::Error),
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
            ApiError::NotFound => StatusCode::NOT_FOUND.into_response(),
            ApiError::InternalServerError(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }
}
