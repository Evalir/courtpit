//! Sign in with Apple / Google: ID-token verification against the providers' JWKS.

use std::{str::FromStr, sync::Arc, time::Duration};

use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header, jwk::JwkSet};
use moka::future::Cache;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

use crate::{ApiError, config::Config};

/// A third-party identity provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema, sqlx::Type)]
#[sqlx(type_name = "auth_provider", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    /// Sign in with Apple.
    Apple,
    /// Google sign-in.
    Google,
}

impl FromStr for Provider {
    type Err = ApiError;
    fn from_str(s: &str) -> Result<Self, ApiError> {
        match s {
            "apple" => Ok(Self::Apple),
            "google" => Ok(Self::Google),
            _ => Err(ApiError::NotFound("identity provider")),
        }
    }
}

/// Where to fetch keys and what to accept for one provider.
#[derive(Debug, Clone)]
pub struct ProviderConfig {
    /// URL of the provider's JSON Web Key Set.
    pub jwks_url: String,
    /// Accepted `iss` values.
    pub issuers: Vec<String>,
    /// Accepted `aud` values (bundle id / service id for Apple, OAuth client ids for Google).
    pub client_ids: Vec<String>,
}

/// The verified claims we use.
#[derive(Debug, Clone)]
pub struct IdClaims {
    /// The provider's stable user id (`sub`).
    pub subject: String,
    /// The trimmed email address, when the token carries a non-empty one.
    pub email: Option<String>,
    /// Whether the provider vouches for ownership of `email`.
    pub email_verified: bool,
}

#[derive(Debug, Deserialize)]
struct RawClaims {
    sub: String,
    email: Option<String>,
    /// Google sends a bool, Apple a `"true"`/`"false"` string.
    email_verified: Option<Value>,
    nonce: Option<String>,
}

/// Verifies ID tokens, caching each provider's JWKS for an hour (refetched on unknown `kid`).
#[derive(Clone)]
pub struct OidcVerifier {
    http: reqwest::Client,
    apple: ProviderConfig,
    google: ProviderConfig,
    keys: Cache<Provider, Arc<JwkSet>>,
}

impl std::fmt::Debug for OidcVerifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OidcVerifier")
            .field("apple", &self.apple)
            .field("google", &self.google)
            .finish_non_exhaustive()
    }
}

impl OidcVerifier {
    /// Builds a verifier from server configuration.
    pub fn from_config(config: &Config) -> Self {
        Self::new(
            ProviderConfig {
                jwks_url: config.apple_jwks_url.clone(),
                issuers: vec!["https://appleid.apple.com".to_owned()],
                client_ids: config.apple_client_ids.clone(),
            },
            ProviderConfig {
                jwks_url: config.google_jwks_url.clone(),
                issuers: vec![
                    "https://accounts.google.com".to_owned(),
                    "accounts.google.com".to_owned(),
                ],
                client_ids: config.google_client_ids.clone(),
            },
        )
    }

    /// Builds a verifier from explicit provider settings.
    pub fn new(apple: ProviderConfig, google: ProviderConfig) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
            apple,
            google,
            keys: Cache::builder().time_to_live(Duration::from_secs(3600)).build(),
        }
    }

    const fn provider(&self, provider: Provider) -> &ProviderConfig {
        match provider {
            Provider::Apple => &self.apple,
            Provider::Google => &self.google,
        }
    }

    async fn fetch_keys(&self, provider: Provider) -> Result<Arc<JwkSet>, ApiError> {
        let url = &self.provider(provider).jwks_url;
        let set: JwkSet = self
            .http
            .get(url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|err| ApiError::Internal(anyhow::anyhow!("fetching JWKS from {url}: {err}")))?
            .json()
            .await
            .map_err(|err| ApiError::Internal(anyhow::anyhow!("parsing JWKS from {url}: {err}")))?;
        let set = Arc::new(set);
        self.keys.insert(provider, Arc::clone(&set)).await;
        Ok(set)
    }

    async fn key_for(&self, provider: Provider, kid: &str) -> Result<DecodingKey, ApiError> {
        let cached = match self.keys.get(&provider).await {
            Some(set) => set,
            None => self.fetch_keys(provider).await?,
        };
        let set = if cached.find(kid).is_some() {
            cached
        } else {
            // Keys rotate: refetch once before giving up.
            self.fetch_keys(provider).await?
        };
        let jwk = set.find(kid).ok_or(ApiError::InvalidCredentials)?;
        DecodingKey::from_jwk(jwk).map_err(|_| ApiError::InvalidCredentials)
    }

    /// Verifies signature, issuer, audience and expiry; checks `nonce` when given.
    pub async fn verify(
        &self,
        provider: Provider,
        id_token: &str,
        nonce: Option<&str>,
    ) -> Result<IdClaims, ApiError> {
        let cfg = self.provider(provider);
        if cfg.client_ids.is_empty() {
            return Err(ApiError::validation(format!("{provider:?} sign-in is not enabled")));
        }
        let header = decode_header(id_token).map_err(|_| ApiError::InvalidCredentials)?;
        if header.alg != Algorithm::RS256 {
            return Err(ApiError::InvalidCredentials);
        }
        let kid = header.kid.ok_or(ApiError::InvalidCredentials)?;
        let key = self.key_for(provider, &kid).await?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_audience(&cfg.client_ids);
        validation.set_issuer(&cfg.issuers);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        validation.leeway = 60;
        let claims = decode::<RawClaims>(id_token, &key, &validation)
            .map_err(|_| ApiError::InvalidCredentials)?
            .claims;
        if let Some(expected) = nonce
            && claims.nonce.as_deref() != Some(expected)
        {
            return Err(ApiError::InvalidCredentials);
        }
        let email_verified = match claims.email_verified {
            Some(Value::Bool(flag)) => flag,
            Some(Value::String(text)) => text == "true",
            _ => false,
        };
        Ok(IdClaims {
            subject: claims.sub,
            email: claims
                .email
                .map(|email| email.trim().to_owned())
                .filter(|email| !email.is_empty()),
            email_verified,
        })
    }
}
