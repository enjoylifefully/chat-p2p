use std::str::FromStr;

use anyhow::Result;
use futures_util::{StreamExt, TryStreamExt};
use iroh::protocol::Router;
use iroh::{Endpoint, SecretKey};
use iroh_gossip::ALPN;
use iroh_gossip::api::{Event, GossipTopic};
use iroh_gossip::net::Gossip;
use iroh_gossip::proto::TopicId;
use tokio::io::{AsyncBufReadExt, BufReader};

#[tokio::main]
async fn main() -> Result<()> {
    // 1) Carregue sua secret_key (ex.: string codificada)
    // Exemplo: let sk = SecretKey::from_str(&std::env::var("IROH_SECRET")?)?;
    let sk = SecretKey::from_bytes(blake3::hash(b"teste").as_bytes());

    // 2) Endpoint com descoberta
    let endpoint = Endpoint::builder()
        .secret_key(sk)
        .discovery_n0()
        .bind()
        .await?;

    // 3) Gossip + router (necessário para aceitar conexões por ALPN do gossip)
    let gossip = Gossip::builder().spawn(endpoint.clone());
    let _router = Router::builder(endpoint.clone())
        .accept(ALPN, gossip.clone())
        .spawn();

    // 4) Entrar no tópico (você pode escolher o nome)
    let topic_id = TopicId::from_bytes(*blake3::hash(b"chat-room-1").as_bytes());
    let topic: GossipTopic = gossip.subscribe_and_join(topic_id, vec![]).await?;
    dbg!(&topic);

    // 5) Split enviar/receber
    let (mut tx, mut rx) = topic.split();

    // leitor de stdin para mandar mensagens
    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();

    // tarefa de recebimento
    tokio::spawn(async move {
        while let Some(Ok(ev)) = rx.next().await {
            if let Event::Received(msg) = ev {
                println!(
                    "[{}] {}",
                    &msg.delivered_from.to_string()[0..8],
                    String::from_utf8_lossy(&msg.content)
                );
            }
        }
    });

    // loop de envio
    while let Some(line) = lines.next_line().await? {
        if line.is_empty() {
            continue;
        }
        tx.broadcast(line.into()).await?;
    }

    Ok(())
}
