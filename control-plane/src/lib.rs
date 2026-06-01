//! altkey-cloud control plane: accounts, billing, registry, the validation
//! authority, usage, and the OpenAPI/Swagger contract the React app generates from.
//! Modules are added in subsequent Foundation tasks (config, db, entities, routes,
//! app, openapi, error, state).
pub mod config;
pub mod db;
pub mod entities;
pub mod error;
pub mod state;
pub mod routes;
pub mod app;
pub mod openapi;
