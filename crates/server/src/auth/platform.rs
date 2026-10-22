//! The platform operator: whoever holds `RACQUETCOLLECTIVE_PLATFORM_TOKEN`, for work above any
//! one community, such as creating communities. Signing in cannot serve here, since a session
//! belongs to a community and the first community does not exist yet.

use std::{fmt, str::FromStr};

use axum::{
    extract::FromRequestParts,
    http::{HeaderMap, request::Parts},
};
use axum_extra::headers::{Authorization, HeaderMapExt, authorization::Bearer};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::{ApiError, AppState};

/// The shared secret of the platform operator, at least [`Self::MIN_LEN`] characters. `Debug`
/// never shows it.
///
/// ```
/// # use racquetcollective_server::auth::PlatformToken;
/// let token: PlatformToken = "0123456789abcdef0123456789abcdef".parse().unwrap();
/// assert!(token.matches("0123456789abcdef0123456789abcdef"));
/// assert!("too short".parse::<PlatformToken>().is_err());
/// ```
#[derive(Clone)]
pub struct PlatformToken(String);

impl PlatformToken {
    /// The fewest characters a token may have.
    pub const MIN_LEN: usize = 32;

    /// Whether `presented` is this token, in time that does not depend on where they differ.
    pub fn matches(&self, presented: &str) -> bool {
        Sha256::digest(self.0.as_bytes()).ct_eq(&Sha256::digest(presented.as_bytes())).into()
    }
}

impl FromStr for PlatformToken {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let token = text.trim();
        if token.chars().count() < Self::MIN_LEN {
            return Err(format!("the platform token needs at least {} characters", Self::MIN_LEN));
        }
        Ok(Self(token.to_owned()))
    }
}

impl fmt::Debug for PlatformToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PlatformToken(..)")
    }
}

/// A request that presented the configured [`PlatformToken`] as its bearer token. Without a
/// configured token the platform routes do not exist (`404`).
#[derive(Debug, Clone, Copy)]
pub struct PlatformOperator;

impl PlatformOperator {
    fn check(token: Option<&PlatformToken>, headers: &HeaderMap) -> Result<Self, ApiError> {
        let token = token.ok_or(ApiError::NotFound("route"))?;
        headers
            .typed_get::<Authorization<Bearer>>()
            .filter(|Authorization(bearer)| token.matches(bearer.token()))
            .map(|_| Self)
            .ok_or(ApiError::Unauthorized)
    }
}

impl FromRequestParts<AppState> for PlatformOperator {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        Self::check(state.config.platform_token.as_ref(), &parts.headers)
    }
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    fn bearer(value: &'static str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        let _ = headers.insert("authorization", HeaderValue::from_static(value));
        headers
    }

    #[test]
    fn short_tokens_are_refused_and_whitespace_trimmed() {
        let _ = "x".repeat(31).parse::<PlatformToken>().unwrap_err();
        let token: PlatformToken = format!("  {TOKEN}\n").parse().unwrap();
        assert!(token.matches(TOKEN));
        assert_eq!(format!("{token:?}"), "PlatformToken(..)");
    }

    #[test]
    fn only_the_configured_bearer_token_passes() {
        let token: PlatformToken = TOKEN.parse().unwrap();
        let _ = PlatformOperator::check(
            Some(&token),
            &bearer("Bearer 0123456789abcdef0123456789abcdef"),
        )
        .unwrap();
        assert!(matches!(
            PlatformOperator::check(
                Some(&token),
                &bearer("Bearer 0123456789abcdef0123456789abcdeX")
            ),
            Err(ApiError::Unauthorized)
        ));
        assert!(matches!(
            PlatformOperator::check(Some(&token), &HeaderMap::new()),
            Err(ApiError::Unauthorized)
        ));
        assert!(matches!(
            PlatformOperator::check(None, &bearer("Bearer 0123456789abcdef0123456789abcdef")),
            Err(ApiError::NotFound(_))
        ));
    }
}
