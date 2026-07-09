use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use soroban_verify_common::Error;

/// Wraps `common::Error` so it can be returned straight from handlers.
pub struct ApiError(pub Error);

pub type ApiResult<T> = std::result::Result<T, ApiError>;

impl<E: Into<Error>> From<E> for ApiError {
    fn from(e: E) -> Self {
        ApiError(e.into())
    }
}

impl ApiError {
    pub fn not_found(msg: impl Into<String>) -> Self {
        ApiError(Error::NotFound(msg.into()))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match &self.0 {
            Error::InvalidInput(_) => StatusCode::BAD_REQUEST,
            Error::NotFound(_) => StatusCode::NOT_FOUND,
            Error::Rpc(_) | Error::Http(_) => StatusCode::BAD_GATEWAY,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        if status.is_server_error() {
            tracing::error!(error = %self.0, "request failed");
        }
        (status, Json(json!({ "error": self.0.to_string() }))).into_response()
    }
}
