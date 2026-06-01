//! Builds the axum Router via utoipa-axum's OpenApiRouter so every routed handler
//! contributes its own path to the OpenAPI document — the served spec and the route
//! table cannot drift. The merged OpenApi is exposed at /api-docs/openapi.json and
//! rendered by Swagger UI at /swagger-ui.
use crate::auth::{magic_link, oauth};
use crate::openapi::ApiDoc;
use crate::routes;
use crate::state::AppState;
use axum::routing::{get, post};
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use utoipa_swagger_ui::SwaggerUi;
use crate::adapters::routes::{internal_get_adapter, internal_list_adapters};

pub fn build(state: AppState) -> axum::Router {
    let (router, api) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(routes::health::health))
        .routes(routes!(routes::me::me))
        .routes(routes!(routes::me::logout))
        .routes(routes!(magic_link::request))
        .routes(routes!(magic_link::consume))
        .routes(routes!(crate::billing::webhook::polar_webhook))
        .routes(routes!(crate::billing::routes::checkout))
        .routes(routes!(crate::billing::routes::portal))
        .routes(routes!(crate::billing::routes::subscription))
        // Registry routes — session-gated, mint-once secrets
        .routes(routes!(crate::registry::routes::list_handles))
        .routes(routes!(crate::registry::routes::handle_availability))
        .routes(routes!(crate::registry::routes::create_handle))
        .routes(routes!(crate::registry::routes::delete_handle))
        .routes(routes!(crate::registry::routes::list_agents))
        .routes(routes!(crate::registry::routes::create_agent))
        .routes(routes!(crate::registry::routes::delete_agent))
        .routes(routes!(crate::registry::routes::list_keys))
        .routes(routes!(crate::registry::routes::create_key))
        .routes(routes!(crate::registry::routes::delete_key))
        // Internal validation endpoints — relay + agent-facing
        .routes(routes!(crate::internal::routes::authorize))
        .routes(routes!(crate::internal::routes::key_validate))
        .routes(routes!(crate::internal::routes::heartbeat))
        .routes(routes!(crate::internal::routes::ingest_usage))
        // Usage dashboard reads — session-gated
        .routes(routes!(crate::usage::routes::usage_summary))
        .routes(routes!(crate::usage::routes::usage_records))
        // Adapter catalog — session-gated dashboard view
        .routes(routes!(crate::adapters::routes::list_adapters))
        .split_for_parts();

    // OAuth start/callback are dynamic-path (`/auth/{provider}/...`) and not
    // individually documented as schemas per-provider; mount as plain axum routes.
    // Apple requires response_mode=form_post and POSTs its callback, so we add
    // a dedicated POST /auth/apple/callback handled by oauth::callback_form.
    // The generic GET :provider callback serves Google/Microsoft/GitHub.
    let router = router
        .route("/auth/:provider/start", get(oauth::start))
        .route("/auth/:provider/callback", get(oauth::callback))
        .route("/auth/apple/callback", post(oauth::callback_form))
        // Internal adapter delivery — unauthenticated (manifests are not secret)
        .route("/internal/adapters", get(internal_list_adapters))
        .route("/internal/adapters/:slug", get(internal_get_adapter));

    router
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", api))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
