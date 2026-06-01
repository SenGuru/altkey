//! Shared application state handed to every handler via axum's `State`.
use crate::auth::email::EmailSender;
use crate::auth::oauth::OAuthRegistry;
use crate::billing::polar::PolarClient;
use crate::config::Config;
use sea_orm::DatabaseConnection;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub config: Config,
    pub email: Arc<dyn EmailSender>,
    pub oauth: Arc<OAuthRegistry>,
    pub polar: Arc<dyn PolarClient>,
}
