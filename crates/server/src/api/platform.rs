//! Platform operator routes (`/api/v1/platform/*`): work above any one community, authorized
//! by the platform token ([`PlatformOperator`]) rather than a session.

use axum::{Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    ApiError, ApiResult, AppState,
    auth::PlatformOperator,
    communities::{self, Branding, NewCommunity},
    extract::ApiJson,
};

/// A community to create.
#[derive(Debug, Deserialize, ToSchema)]
pub struct CommunityCreate {
    /// URL-safe identifier: lowercase letters, digits and inner dashes, at most 63 characters.
    pub slug: String,
    /// Display name.
    pub name: String,
    /// A host the community answers on besides `{slug}.{base domain}`.
    pub custom_domain: Option<String>,
    /// Initial theme; defaults apply to missing keys.
    #[serde(default)]
    pub branding: Branding,
    /// The owner: an existing or new user, made verified and given an `owner` player.
    pub owner_email: Option<String>,
}

/// A community just created.
#[derive(Debug, Serialize, ToSchema)]
pub struct CommunityCreated {
    /// Community id.
    pub id: Uuid,
    /// URL-safe identifier.
    pub slug: String,
    /// Display name.
    pub name: String,
    /// Registered custom domain, lowercase.
    pub custom_domain: Option<String>,
    /// The owner's player, when an owner was given.
    pub owner_player_id: Option<Uuid>,
}

/// Creates a community and, optionally, its owner.
#[utoipa::path(
    post,
    path = "/api/v1/platform/communities",
    tag = "platform",
    security(("platform" = [])),
    request_body = CommunityCreate,
    responses(
        (status = 201, body = CommunityCreated),
        (status = 401, description = "Missing or wrong platform token", body = crate::error::ErrorBody),
        (status = 404, description = "No platform token is configured", body = crate::error::ErrorBody),
        (status = 409, description = "The slug or custom domain is taken", body = crate::error::ErrorBody),
    )
)]
pub async fn create_community(
    State(state): State<AppState>,
    _operator: PlatformOperator,
    ApiJson(body): ApiJson<CommunityCreate>,
) -> ApiResult<(StatusCode, Json<CommunityCreated>)> {
    if body.name.trim().is_empty() {
        return Err(ApiError::Validation("`name` must not be empty".to_owned()));
    }
    let created = communities::create_community(
        &state.db,
        NewCommunity {
            slug: body.slug,
            name: body.name,
            custom_domain: body.custom_domain.filter(|domain| !domain.trim().is_empty()),
            branding: body.branding,
            owner_email: body.owner_email.filter(|email| !email.trim().is_empty()),
        },
    )
    .await
    .map_err(creation_error)?;
    let community = created.community;
    Ok((
        StatusCode::CREATED,
        Json(CommunityCreated {
            id: community.id,
            slug: community.slug,
            name: community.name,
            custom_domain: community.custom_domain,
            owner_player_id: created.owner_player_id,
        }),
    ))
}

/// Maps the database's refusals to the caller's mistakes; anything else is internal.
fn creation_error(err: anyhow::Error) -> ApiError {
    let code = err
        .downcast_ref::<sqlx::Error>()
        .and_then(sqlx::Error::as_database_error)
        .and_then(sqlx::error::DatabaseError::code)
        .map(std::borrow::Cow::into_owned);
    match code.as_deref() {
        Some("23505") => ApiError::Conflict("the slug or custom domain is taken".to_owned()),
        Some("23514") => ApiError::Validation(
            "`slug` must be lowercase letters, digits and inner dashes, at most 63 characters"
                .to_owned(),
        ),
        _ => ApiError::Internal(err),
    }
}
