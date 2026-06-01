//! The single utoipa `OpenApi` document. Every documented handler is listed in
//! `paths(...)`; every response/request schema in `components(schemas(...))`.
//! This is the contract the React app generates its client from — keep it complete.
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    info(title = "altkey control plane", version = "0.1.0"),
    paths(crate::routes::health::health),
    components(schemas(crate::routes::health::Health)),
    tags(
        (name = "system", description = "Service + health")
    )
)]
pub struct ApiDoc;
