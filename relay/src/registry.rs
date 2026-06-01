//! Shared relay state: which agent owns which handle, and a pending-connection
//! handoff used to pair a public socket with the agent's data connection.
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};

type ControlTx = mpsc::Sender<u64>;

#[derive(Clone)]
pub struct Registry {
    handles: Arc<Mutex<HashMap<String, ControlTx>>>,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<TcpStream>>>>,
    next_id: Arc<AtomicU64>,
}

impl Registry {
    pub fn new() -> Self {
        Registry {
            handles: Arc::new(Mutex::new(HashMap::new())),
            pending: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(AtomicU64::new(1)),
        }
    }
    pub fn register_handle(&self, handle: String, tx: ControlTx) {
        self.handles.lock().insert(handle, tx);
    }
    pub fn unregister_handle(&self, handle: &str) {
        self.handles.lock().remove(handle);
    }
    pub fn control_for(&self, handle: &str) -> Option<ControlTx> {
        self.handles.lock().get(handle).cloned()
    }
    pub fn reserve_conn(&self) -> (u64, oneshot::Receiver<TcpStream>) {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(id, tx);
        (id, rx)
    }
    pub fn take_pending(&self, conn_id: u64) -> Option<oneshot::Sender<TcpStream>> {
        self.pending.lock().remove(&conn_id)
    }
    pub async fn run(self, public_addr: &str, agent_addr: &str) -> anyhow::Result<()> {
        let agent = crate::agent_conn::serve(self.clone(), agent_addr.to_string());
        let public = crate::public::serve(self.clone(), public_addr.to_string());
        tokio::try_join!(agent, public)?;
        Ok(())
    }
}
