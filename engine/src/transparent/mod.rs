pub mod ca;
pub mod hosts;
pub mod server;
pub mod trust;

use anyhow::Result;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

static ENABLED: AtomicBool = AtomicBool::new(false);

/// Turn transparent mode ON: ensure CA, trust it, redirect hosts, set the
/// per-request bypass, and start the :443 intercept server serving `app`.
/// Requires admin/root (hosts edit, trust install, :443 bind).
pub async fn enable(app: axum::Router) -> Result<()> {
    let ca = server::load_ca()?;
    trust::install(&crate::config::ca_cert_path())?;
    hosts::enable(&crate::config::hosts_path(), &crate::config::INTERCEPT_HOSTS[..])?;
    std::env::set_var("ALTKEY_TRANSPARENT", "1");
    let _task = server::serve(app, Arc::clone(&ca)).await?;
    ENABLED.store(true, Ordering::SeqCst);
    std::mem::forget(_task);
    Ok(())
}

/// Turn transparent mode OFF: remove the hosts redirect + untrust the CA.
pub fn disable() -> Result<()> {
    hosts::disable(&crate::config::hosts_path())?;
    trust::uninstall()?;
    std::env::remove_var("ALTKEY_TRANSPARENT");
    ENABLED.store(false, Ordering::SeqCst);
    Ok(())
}

pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::SeqCst)
}
