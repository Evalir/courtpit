//! The OpenAPI document's shared parts: info, tags, security schemes and error conventions.
//!
//! Handlers declare only what is specific to them (`#[utoipa::path]`). Everything that is the
//! same for every operation is applied here, once, so it cannot drift:
//!
//! - an operation with `security(("bearer" = []))` also accepts the session cookie;
//! - every operation has a `default` response with the shared [`ErrorBody`] (clients switch on
//!   `error.code`); secured operations also have a `401` and operations with a body a `422`
//!   (the extractors answer both before the handler runs);
//! - responses declared without a description get the status's standard reason phrase.

use axum::http::StatusCode;
use utoipa::{
    Modify, OpenApi,
    openapi::{
        Content, OpenApi as Document, Ref, RefOr, Response,
        path::{Operation, PathItem},
        security::{
            ApiKey, ApiKeyValue, HttpAuthScheme, HttpBuilder, SecurityRequirement, SecurityScheme,
        },
    },
};

use crate::{auth::SESSION_COOKIE, error::ErrorBody};

/// Name of the bearer-token security scheme (what handlers put in `security(...)`).
const BEARER: &str = "bearer";
/// Name of the session-cookie security scheme (added next to [`BEARER`] by [`ApiConventions`]).
const SESSION_COOKIE_SCHEME: &str = "session_cookie";

const DESCRIPTION: &str = "Racquet Collective REST API.\n\n\
**Community.** Every request is scoped to one community. Native clients send \
`X-RacquetCollective-Community: <slug>`; browsers are resolved from the host (`<slug>.racquetcollective.app` or a \
registered custom domain). The header wins over the host. A request without a resolvable \
community answers `400`, an unknown one `404`.\n\n\
**Authentication.** A session is an opaque token, sent as `Authorization: Bearer <token>` \
(native) or as the `racquetcollective_session` cookie (web; ask for it at sign-in with \
`X-RacquetCollective-Client: web`). Either one authenticates.\n\n\
**Errors.** Every error is `{ \"error\": { \"code\", \"message\" } }`; clients switch on `code`.\n\n\
**Lists** are cursor-paginated: `?cursor=&limit=` in, `{ items, next_cursor }` out.";

/// The document every route registers into; [`ApiConventions`] finishes it.
#[derive(OpenApi)]
#[openapi(
    info(title = "Racquet Collective API", version = "0.1.0", description = DESCRIPTION),
    components(schemas(ErrorBody)),
    tags(
        (name = "health", description = "Liveness and readiness probes."),
        (name = "tenant", description = "The community a request resolves to."),
        (name = "auth", description = "Sign-in with email codes, passwords or Apple/Google."),
        (name = "me", description = "The signed-in player's profile and account."),
        (name = "players", description = "The community's player directory."),
        (name = "matches", description = "Matches, scheduling proposals and results."),
        (name = "match-requests", description = "Open requests for a game."),
        (name = "leagues", description = "Leagues, registration and standings."),
        (name = "rankings", description = "Rankings and their points ledger."),
        (name = "admin", description = "Community administration (admins only)."),
    )
)]
pub(crate) struct ApiDoc;

/// Declares the security schemes and applies the error conventions to every operation. It must
/// run on the finished document: utoipa's own `modifiers(..)` run before the routes (and so
/// the operations) are merged in.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ApiConventions;

impl Modify for ApiConventions {
    fn modify(&self, api: &mut Document) {
        let components = api.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            BEARER,
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .description(Some("Session token (native clients)."))
                    .build(),
            ),
        );
        components.add_security_scheme(
            SESSION_COOKIE_SCHEME,
            SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::with_description(
                SESSION_COOKIE,
                "Session cookie (web clients); alternative to the bearer token.",
            ))),
        );
        for item in api.paths.paths.values_mut() {
            for operation in operations(item) {
                apply_conventions(operation);
            }
        }
    }
}

/// Every operation of a path item.
fn operations(item: &mut PathItem) -> impl Iterator<Item = &mut Operation> {
    [
        &mut item.get,
        &mut item.put,
        &mut item.post,
        &mut item.delete,
        &mut item.options,
        &mut item.head,
        &mut item.patch,
        &mut item.trace,
    ]
    .into_iter()
    .flatten()
}

fn apply_conventions(operation: &mut Operation) {
    if let Some(requirements) = operation
        .security
        .as_mut()
        .filter(|requirements| !requirements.is_empty())
    {
        requirements.push(SecurityRequirement::new(
            SESSION_COOKIE_SCHEME,
            Vec::<String>::new(),
        ));
        ensure_error(operation, "401", "Missing, invalid or expired session.");
    }
    if operation.request_body.is_some() {
        ensure_error(operation, "422", "The body is malformed or breaks a rule.");
    }
    ensure_error(operation, "default", "Any other error; see `error.code`.");
    for (status, response) in &mut operation.responses.responses {
        if let RefOr::T(response) = response
            && response.description.is_empty()
        {
            response.description = reason(status);
        }
    }
}

/// Adds an [`ErrorBody`] response for `status` unless the handler declared one.
fn ensure_error(operation: &mut Operation, status: &str, description: &str) {
    let _ = operation
        .responses
        .responses
        .entry(status.to_owned())
        .or_insert_with(|| error_response(description).into());
}

/// A response carrying the shared [`ErrorBody`].
fn error_response(description: &str) -> Response {
    Response::builder()
        .description(description)
        .content(
            "application/json",
            Content::new(Some(Ref::from_schema_name("ErrorBody"))),
        )
        .build()
}

/// The standard reason phrase for a status key (`"404"` becomes `"Not Found"`).
fn reason(status: &str) -> String {
    status
        .parse::<u16>()
        .ok()
        .and_then(|code| StatusCode::from_u16(code).ok())
        .and_then(|code| code.canonical_reason())
        .unwrap_or("Response")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;
    use crate::{app::openapi, auth::CLIENT_HEADER, tenancy::COMMUNITY_HEADER};

    const METHODS: [&str; 8] = [
        "get", "put", "post", "delete", "options", "head", "patch", "trace",
    ];

    fn document() -> Value {
        serde_json::to_value(openapi()).expect("the document serializes")
    }

    /// `(path, method, operation)` for every operation in the document.
    fn operations_of(doc: &Value) -> Vec<(&str, &str, &Value)> {
        let paths = doc["paths"].as_object().expect("paths is an object");
        paths
            .iter()
            .flat_map(|(path, item)| {
                METHODS
                    .iter()
                    .filter_map(move |method| Some((path.as_str(), *method, item.get(*method)?)))
            })
            .collect()
    }

    fn collect_refs<'doc>(value: &'doc Value, out: &mut Vec<&'doc str>) {
        match value {
            Value::Object(map) => {
                for (key, inner) in map {
                    match inner.as_str() {
                        Some(target) if key == "$ref" => out.push(target),
                        _ => collect_refs(inner, out),
                    }
                }
            }
            Value::Array(items) => items.iter().for_each(|item| collect_refs(item, out)),
            _ => {}
        }
    }

    /// The description spells out header and cookie names; keep them in step with the code.
    #[test]
    fn description_names_the_wire_names() {
        let description = DESCRIPTION.to_lowercase();
        for name in [COMMUNITY_HEADER, CLIENT_HEADER, SESSION_COOKIE] {
            assert!(
                description.contains(name),
                "{name} missing from DESCRIPTION"
            );
        }
    }

    #[test]
    fn every_operation_has_an_id_and_a_declared_tag() {
        let doc = document();
        let declared: Vec<&str> = doc["tags"]
            .as_array()
            .expect("tags are declared")
            .iter()
            .filter_map(|tag| tag["name"].as_str())
            .collect();
        let mut ids = std::collections::HashSet::new();
        for (path, method, operation) in operations_of(&doc) {
            let id = operation["operationId"].as_str();
            assert!(
                id.is_some_and(|id| ids.insert(id)),
                "{method} {path}: missing or duplicate operationId"
            );
            let tags = operation["tags"].as_array().map(Vec::as_slice);
            assert!(
                tags.is_some_and(|tags| !tags.is_empty()
                    && tags
                        .iter()
                        .all(|tag| tag.as_str().is_some_and(|tag| declared.contains(&tag)))),
                "{method} {path}: needs tags declared on ApiDoc"
            );
        }
    }

    #[test]
    fn success_responses_declare_their_bodies() {
        let doc = document();
        for (path, method, operation) in operations_of(&doc) {
            let successes: Vec<(&String, &Value)> = operation["responses"]
                .as_object()
                .expect("responses is an object")
                .iter()
                .filter(|(status, _)| status.starts_with('2'))
                .collect();
            assert!(!successes.is_empty(), "{method} {path}: no 2xx response");
            for (status, response) in successes {
                let content = &response["content"]["application/json"];
                match status.as_str() {
                    "204" => assert!(
                        response.get("content").is_none(),
                        "{method} {path}: 204 must not have a body"
                    ),
                    "202" => {}
                    _ => assert!(
                        content["schema"].is_object(),
                        "{method} {path}: {status} needs an application/json schema"
                    ),
                }
            }
        }
    }

    #[test]
    fn every_operation_documents_its_errors_and_security() {
        let doc = document();
        let schemes = doc["components"]["securitySchemes"]
            .as_object()
            .expect("security schemes are declared");
        for (path, method, operation) in operations_of(&doc) {
            let error_ref =
                &operation["responses"]["default"]["content"]["application/json"]["schema"]["$ref"];
            assert_eq!(
                error_ref, "#/components/schemas/ErrorBody",
                "{method} {path}: default response must be the shared ErrorBody"
            );
            let Some(requirements) = operation["security"].as_array() else {
                continue;
            };
            assert!(
                operation["responses"]["401"].is_object(),
                "{method} {path}: secured operations document 401"
            );
            for requirement in requirements {
                for name in requirement.as_object().expect("requirement").keys() {
                    assert!(schemes.contains_key(name), "{method} {path}: scheme {name}");
                }
            }
            assert_eq!(requirements.len(), 2, "{method} {path}: bearer or cookie");
        }
    }

    /// Path parameters must match the template both ways: a missing one leaves a hole in the
    /// client, and a query struct whose `IntoParams` lacks `parameter_in = Query` shows up as
    /// a "path" parameter the template doesn't have.
    #[test]
    fn path_parameters_match_the_template() {
        let doc = document();
        for (path, method, operation) in operations_of(&doc) {
            let mut documented: Vec<&str> = operation["parameters"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .filter(|param| param["in"] == "path")
                .filter_map(|param| param["name"].as_str())
                .collect();
            let mut templated: Vec<&str> = path
                .split('/')
                .filter_map(|segment| segment.strip_prefix('{')?.strip_suffix('}'))
                .collect();
            documented.sort_unstable();
            templated.sort_unstable();
            assert_eq!(documented, templated, "{method} {path}: path parameters");
        }
    }

    #[test]
    fn every_ref_resolves() {
        let doc = document();
        let mut refs = Vec::new();
        collect_refs(&doc, &mut refs);
        assert!(!refs.is_empty());
        for target in refs {
            let pointer = target.strip_prefix('#').expect("local reference");
            assert!(
                doc.pointer(pointer).is_some(),
                "{target} does not resolve to a component"
            );
        }
    }
}
