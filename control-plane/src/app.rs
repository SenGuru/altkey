//! Builds the axum Router: shared state, routes, middleware, and the OpenAPI
//! contract surface (`/api-docs/openapi.json` + `/swagger-ui`).
use crate::openapi::ApiDoc;
use crate::routes;
use crate::state::AppState;
use axum::routing::get;
use axum::Router;
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

pub fn build(state: AppState) -> Router {
    // SwaggerUi implements `From<SwaggerUi> for Router<S>` (generic over state S),
    // so we convert explicitly to Router<AppState> before merging. This keeps all
    // state types unified before the final `.with_state()` call.
    let swagger: Router<AppState> =
        Router::from(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()));

    Router::new()
        .route("/health", get(routes::health::health))
        .merge(swagger)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
