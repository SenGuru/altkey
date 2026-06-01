//! Control-channel messages between agent and relay, framed as a u32
//! length-prefix + JSON body. Small, human-debuggable, order-preserving.
use anyhow::{bail, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
pub enum AgentMsg {
    Hello { handle: String, token: String },
    Data { conn_id: u64 },
}

#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
pub enum RelayMsg {
    Ready,
    Reject { reason: String },
    Open { conn_id: u64 },
}

const MAX_FRAME: u32 = 64 * 1024;

pub async fn write_msg<W: AsyncWriteExt + Unpin, T: serde::Serialize>(w: &mut W, msg: &T) -> Result<()> {
    let body = serde_json::to_vec(msg)?;
    if body.len() as u32 > MAX_FRAME {
        bail!("frame too large: {}", body.len());
    }
    w.write_all(&(body.len() as u32).to_be_bytes()).await?;
    w.write_all(&body).await?;
    w.flush().await?;
    Ok(())
}

pub async fn read_msg<R: AsyncReadExt + Unpin, T: serde::de::DeserializeOwned>(r: &mut R) -> Result<T> {
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf);
    if len > MAX_FRAME {
        bail!("incoming frame too large: {len}");
    }
    let mut body = vec![0u8; len as usize];
    r.read_exact(&mut body).await?;
    Ok(serde_json::from_slice(&body)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn agent_msg_round_trips_over_a_duplex() {
        let (mut a, mut b) = tokio::io::duplex(1024);
        let sent = AgentMsg::Hello { handle: "sen".into(), token: "tok".into() };
        write_msg(&mut a, &sent).await.unwrap();
        let got: AgentMsg = read_msg(&mut b).await.unwrap();
        assert_eq!(sent, got);
    }

    #[tokio::test]
    async fn relay_msg_open_round_trips() {
        let (mut a, mut b) = tokio::io::duplex(1024);
        write_msg(&mut a, &RelayMsg::Open { conn_id: 42 }).await.unwrap();
        let got: RelayMsg = read_msg(&mut b).await.unwrap();
        assert_eq!(got, RelayMsg::Open { conn_id: 42 });
    }
}
