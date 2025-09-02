use std::collections::HashSet;
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use blake3;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use futures_lite::StreamExt;
use iroh::NodeId;
use mainline::Id;
use mainline::async_dht::AsyncDht;
use serde::{Deserialize, Serialize};
use tokio::net::UdpSocket;

use crate::error::{DiscoveryError, PostcardError, SignatureError};

pub type Nonce = [u8; 16];

pub fn infohash_for(topic: &str) -> Id {
    let hash = blake3::hash(topic.as_bytes());
    let infohash = &hash.as_bytes()[..20];

    dbg!(&hash);
    println!("{hash}");
    Id::from_bytes(infohash).unwrap()
}

#[derive(Serialize, Deserialize)]
struct WhoAmI {
    client_nonce: Nonce,
    client_key: VerifyingKey,
    client_sig: Signature,
}

#[derive(Serialize, Deserialize)]
struct WhoAmIResp {
    client_nonce: Nonce,
    server_key: VerifyingKey,
    server_sig: Signature,
}

impl WhoAmI {
    fn from_buf(buf: &mut [u8]) -> Result<WhoAmI, PostcardError> {
        postcard::from_bytes_cobs(buf)
    }

    fn to_bytes(&self) -> Vec<u8> {
        postcard::to_allocvec(self).unwrap()
    }

    fn into_resp(self, sig_key: &SigningKey) -> Result<WhoAmIResp, SignatureError> {
        self.client_key
            .verify_strict(&self.client_nonce, &self.client_sig)?;

        let sig = sig_key.sign(&self.client_nonce);

        Ok(WhoAmIResp {
            client_nonce: self.client_nonce,
            server_key: sig_key.verifying_key(),
            server_sig: sig,
        })
    }
}

impl WhoAmIResp {
    fn from_buf(buf: &mut [u8]) -> Result<WhoAmIResp, PostcardError> {
        postcard::from_bytes_cobs(buf)
    }

    fn to_bytes(&self) -> Vec<u8> {
        postcard::to_allocvec(self).unwrap()
    }

    fn verify(&self, nonce: Nonce) -> Result<(), DiscoveryError> {
        if self.client_nonce != nonce {
            return Err(DiscoveryError::InvalidNonce);
        }

        self.server_key
            .verify_strict(&nonce, &self.server_sig)
            .map_err(DiscoveryError::from)
    }
}

pub async fn run_whoami_server(sig_key: SigningKey, port: u16) -> Result<(), DiscoveryError> {
    let socket_addr = SocketAddr::from((Ipv4Addr::UNSPECIFIED, port));
    let socket = UdpSocket::bind(socket_addr).await?;
    let mut buf = [0u8; 1500];

    loop {
        let (buf_len, addr) = socket.recv_from(&mut buf).await?;

        let Ok(whoai) = WhoAmI::from_buf(&mut buf) else {
            continue;
        };
        let Ok(resp) = whoai.into_resp(&sig_key) else {
            continue;
        };

        socket.send_to(&resp.to_bytes(), addr).await?;
    }
}

/// anuncia regularmente no infohash do protocolo
pub async fn dht_reannounce_loop(dht: AsyncDht, infohash: Id, port: u16, period: Duration) {
    loop {
        if let Err(e) = dht.announce_peer(infohash, Some(port)).await {
            eprintln!("[dht] announce error: {e:?}");
        }
        tokio::time::sleep(period).await;
    }
}

/// coleta um conjunto de peers (IP:porta) para um tópico
pub async fn dht_collect_peers(
    dht: AsyncDht,
    infohash: &Id,
    target: usize,
    timeout: Duration,
) -> Vec<SocketAddr> {
    let mut out_set = HashSet::new();
    let mut stream = dht.get_peers(*infohash);
    let deadline = tokio::time::Instant::now() + timeout;

    while let Some(batch) = tokio::time::timeout(
        deadline.saturating_duration_since(tokio::time::Instant::now()),
        stream.next(),
    )
    .await
    .unwrap_or(None)
    {
        for peer in batch {
            out_set.insert(SocketAddr::from(peer));
        }
        if out_set.len() >= target {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            break;
        }
    }
    out_set.into_iter().collect()
}

pub async fn probe_peer(
    addr: SocketAddr,
    timeout: Duration,
    sig_key: &SigningKey,
) -> Result<NodeId, DiscoveryError> {
    let socket = UdpSocket::bind(("0.0.0.0", 0)).await?;
    let nonce = rand::random::<Nonce>();
    let whoami = WhoAmI {
        client_nonce: nonce,
        client_key: sig_key.verifying_key(),
        client_sig: sig_key.sign(&nonce),
    };
    let bytes = whoami.to_bytes();

    socket.send_to(&bytes, addr).await?;

    let mut buf = [0u8; 1500];

    tokio::time::timeout(timeout, socket.recv_from(&mut buf))
        .await
        .map_err(|_| DiscoveryError::Timeout)??;

    let resp = WhoAmIResp::from_buf(&mut buf)?;

    resp.verify(nonce)?;

    Ok(NodeId::from(resp.server_key))
}
