//! Builds the axum Router: shared state, routes, middleware. The Swagger UI +
//! OpenAPI JSON are merged in Task 7 so this is the single route-table source.
use crate::routes;
use crate::state::AppState;
use axum::routing::get;
use axum::Router;
use tower_http::trace::TraceLayer;

pub fn build(state: AppState) -> Router {
    Router::new()
        .route("/health", get(routes::health::health))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
