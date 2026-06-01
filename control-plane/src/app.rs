//! Builds the axum Router via utoipa-axum's OpenApiRouter so every routed handler
//! contributes its own path to the OpenAPI document — the served spec and the route
//! table cannot drift. The merged OpenApi is exposed at /api-docs/openapi.json and
//! rendered by Swagger UI at /swagger-ui.
use crate::openapi::ApiDoc;
use crate::routes;
use crate::state::AppState;
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use utoipa_swagger_ui::SwaggerUi;

pub fn build(state: AppState) -> axum::Router {
    let (router, api) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(routes::health::health))
        .split_for_parts();

    router
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", api))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
