//! altkey-cloud control plane: accounts, billing, registry, the validation
//! authority, usage, and the OpenAPI/Swagger contract the React app generates from.
//! Modules are added in subsequent Foundation tasks (config, db, entities, routes,
//! app, openapi, error, state).
pub mod auth;
pub mod billing;
pub mod config;
pub mod db;
pub mod entities;
pub mod error;
pub mod internal;
pub mod registry;
pub mod state;
pub mod routes;
pub mod usage;
pub mod adapters;
pub mod app;
pub mod openapi;
