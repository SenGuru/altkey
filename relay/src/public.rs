//! Public listener — implemented in Task 6.
use crate::registry::Registry;
pub async fn serve(_reg: Registry, _addr: String) -> anyhow::Result<()> { Ok(()) }
