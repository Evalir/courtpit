//! The single API error type, rendered as `{ "error": { "code", "message" } }`.

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use utoipa::ToSchema;

/// Result alias for handlers.
pub type ApiResult<T> = Result<T, ApiError>;

/// Every error a handler can return. The variant decides the HTTP status and the stable
/// machine-readable `code`; the `Display` text becomes the `message`.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// Malformed request (bad header, unparsable input).
    #[error("{0}")]
    BadRequest(String),
    /// Well-formed input that breaks a rule (invalid score, bad date range, ...).
    #[error("{0}")]
    Validation(String),
    /// Missing or invalid credentials.
    #[error("authentication required")]
    Unauthorized,
    /// Wrong or expired code, or wrong email/password.
    #[error("invalid or expired credentials")]
    InvalidCredentials,
    /// Authenticated but not allowed.
    #[error("{0}")]
    Forbidden(String),
    /// The named resource does not exist (or is not visible to the caller).
    #[error("{0} not found")]
    NotFound(&'static str),
    /// The request conflicts with current state (duplicate, wrong status, ...).
    #[error("{0}")]
    Conflict(String),
    /// A conflict clients must tell apart from other conflicts, so it carries its own stable
    /// `code` (e.g. `season_not_over`).
    #[error("{message}")]
    ConflictCode {
        /// The stable machine-readable code.
        code: &'static str,
        /// Human-readable explanation.
        message: String,
    },
    /// Rate limit exceeded.
    #[error("too many requests, slow down")]
    RateLimited,
    /// Unexpected failure; details are logged, not returned.
    #[error("internal error")]
    Internal(#[source] anyhow::Error),
}

impl ApiError {
    /// HTTP status for this error.
    pub const fn status(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Validation(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Unauthorized | Self::InvalidCredentials => StatusCode::UNAUTHORIZED,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Conflict(_) | Self::ConflictCode { .. } => StatusCode::CONFLICT,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Stable machine-readable code; clients switch on it.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::BadRequest(_) => "bad_request",
            Self::Validation(_) => "validation_failed",
            Self::Unauthorized => "unauthorized",
            Self::InvalidCredentials => "invalid_credentials",
            Self::Forbidden(_) => "forbidden",
            Self::NotFound(_) => "not_found",
            Self::Conflict(_) => "conflict",
            Self::ConflictCode { code, .. } => code,
            Self::RateLimited => "rate_limited",
            Self::Internal(_) => "internal",
        }
    }

    /// Shorthand for [`ApiError::Validation`].
    pub fn validation(msg: impl Into<String>) -> Self {
        Self::Validation(msg.into())
    }

    /// Shorthand for [`ApiError::Conflict`].
    pub fn conflict(msg: impl Into<String>) -> Self {
        Self::Conflict(msg.into())
    }

    /// Shorthand for [`ApiError::ConflictCode`].
    pub fn conflict_code(code: &'static str, message: impl Into<String>) -> Self {
        Self::ConflictCode { code, message: message.into() }
    }

    /// Shorthand for [`ApiError::Forbidden`].
    pub fn forbidden(msg: impl Into<String>) -> Self {
        Self::Forbidden(msg.into())
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(err: sqlx::Error) -> Self {
        match err {
            sqlx::Error::RowNotFound => Self::NotFound("resource"),
            other => Self::Internal(other.into()),
        }
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(err: anyhow::Error) -> Self {
        Self::Internal(err)
    }
}

/// The JSON body of every error response.
#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorBody {
    /// What went wrong.
    pub error: ErrorDetail,
}

/// Machine-readable code plus a human-readable message.
#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorDetail {
    /// Stable machine-readable code; clients switch on it. One of `bad_request` (400),
    /// `validation_failed` (422), `unauthorized` and `invalid_credentials` (401), `forbidden`
    /// (403), `not_found` (404), `conflict` (409), `rate_limited` (429) or `internal` (500).
    pub code: String,
    /// Human-readable explanation; never contains internal details.
    pub message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        if let Self::Internal(err) = &self {
            tracing::error!(error = ?err, "internal error");
        }
        let body = ErrorBody {
            error: ErrorDetail { code: self.code().to_owned(), message: self.to_string() },
        };
        (status, Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_errors_hide_details() {
        let err = ApiError::Internal(anyhow::anyhow!("db password is hunter2"));
        assert_eq!(err.to_string(), "internal error");
        assert_eq!(err.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn error_body_shape() {
        use http_body_util::BodyExt;
        let res = ApiError::NotFound("match").into_response();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            body,
            serde_json::json!({ "error": { "code": "not_found", "message": "match not found" } })
        );
    }
}
