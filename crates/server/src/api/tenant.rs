//! `GET /api/v1/tenant`: branding for the client to theme itself at boot.

use axum::Json;
use serde::Serialize;
use utoipa::ToSchema;

use crate::{communities::Branding, tenancy::Tenant};

/// Public description of the community the request resolved to.
#[derive(Debug, Serialize, ToSchema)]
pub struct TenantInfo {
    /// URL-safe community identifier.
    pub slug: String,
    /// Display name.
    pub name: String,
    /// Client theme.
    pub branding: Branding,
}

/// Returns the current community's branding.
#[utoipa::path(
    get,
    path = "/api/v1/tenant",
    tag = "tenant",
    responses(
        (status = 200, body = TenantInfo),
        (status = 400, description = "No community in request", body = crate::error::ErrorBody),
        (status = 404, description = "Unknown community", body = crate::error::ErrorBody),
    )
)]
pub async fn get_tenant(tenant: Tenant) -> Json<TenantInfo> {
    Json(TenantInfo {
        slug: tenant.slug.clone(),
        name: tenant.name.clone(),
        branding: tenant.branding.0.clone(),
    })
}
