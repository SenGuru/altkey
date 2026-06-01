//! Base OpenAPI document: info, tags, and (later) security schemes. Concrete
//! paths + schemas are contributed by each route module via `OpenApiRouter`, so a
//! routed endpoint cannot be missing from the served spec.
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    info(title = "altkey control plane", version = "0.1.0"),
    tags(
        (name = "system", description = "Service + health"),
        (name = "auth", description = "Login, sessions, identity")
    )
)]
pub struct ApiDoc;
