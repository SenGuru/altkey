//! altkey engine library crate. Exposes the same modules the binary uses so
//! integration tests (and Task 7) can `use altkey::...`. The binary (`main.rs`)
//! is a separate crate that compiles the same source files via its own `mod`
//! lines; lib and bin coexist.
pub mod config;
pub mod store;
pub mod auth;
pub mod sse;
pub mod translate;
pub mod providers;
pub mod routes;
pub mod transparent;
