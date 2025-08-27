mod chat_event;
mod chat_ticket;

use std::collections::HashMap;
use std::io::Write;
use std::ops::Not;
use std::str::FromStr;

use anyhow::Result;
use clap::Parser;
use futures_lite::StreamExt;
use iroh::protocol::Router;
use iroh::{Endpoint, NodeAddr, NodeId, PublicKey, Watcher};
use iroh_gossip::api::{Event, GossipReceiver, Message};
use iroh_gossip::net::Gossip;
use iroh_gossip::proto::TopicId;
use rustyline_async::{Readline, ReadlineEvent, SharedWriter};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncWriteExt, stdout};
use {base58, postcard, thiserror};

use crate::chat_event::{ChatEvent, ChatEventBuilder, SignedChatEvent};
use crate::chat_ticket::ChatTicket;

/// Chat over iroh-gossip
///
/// This broadcasts unsigned messages over iroh-gossip.
///
/// By default a new node id is created when starting the example.
///
/// By default, we use the default n0 discovery services to dial by `NodeId`.
#[derive(Parser, Debug)]
struct Args {
    /// Set your nickname.
    #[clap(short, long)]
    name: Option<String>,
    /// Set the bind port for our socket. By default, a random port will be used.
    #[clap(short, long, default_value = "0")]
    bind_port: u16,
    #[clap(subcommand)]
    command: Command,
}

#[derive(Parser, Debug)]
enum Command {
    /// Open a chat room for a topic and print a ticket for others to join.
    Open,
    /// Join a chat room from a ticket.
    Join {
        /// The ticket, as base32 string.
        ticket: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let (topic, nodes) = match &args.command {
        Command::Open => {
            let topic = TopicId::from_bytes(rand::random());
            //writeln!(w, "topic:   {topic}")?;
            (topic, vec![])
        }
        Command::Join { ticket } => {
            let ChatTicket { topic, nodes } = ChatTicket::from_str(ticket)?;
            //writeln!(w, "| topic:  {topic}")?;
            (topic, nodes)
        }
    };

    let endpoint = Endpoint::builder().discovery_n0().bind().await?;

    // writeln!(w, "| :   {}", endpoint.secret_key().public())?;

    let gossip = Gossip::builder().spawn(endpoint.clone());

    let router = Router::builder(endpoint.clone())
        .accept(iroh_gossip::ALPN, gossip.clone())
        .spawn();

    let ticket = {
        let me = endpoint.node_addr().get().unwrap();
        let nodes = vec![me];
        ChatTicket { topic, nodes }
    };

    let (mut rl, mut w) = Readline::new("> ".to_string())?;

    writeln!(w, "ticket: {ticket}")?;

    let node_ids = nodes.iter().map(|p| p.node_id).collect();
    for node in nodes.into_iter() {
        endpoint.add_node_addr(node)?;
    }

    let (sender, receiver) = gossip.subscribe(topic, node_ids).await?.split();

    writeln!(w, "connected")?;

    rl.should_print_line_on(false, false);
    rl.clear()?;

    tokio::spawn(subscribe_loop(receiver, w.clone()));

    let key = endpoint.secret_key().secret();

    while let Ok(line_event) = rl.readline().await {
        let ReadlineEvent::Line(line) = line_event else {
            break;
        };

        let event = if line.starts_with("/") {
            let (action, rest) = match line.split_once(char::is_whitespace) {
                Some((cmd, rest)) => (cmd, rest.trim_start()),
                None => (line.as_str(), ""),
            };

            match action {
                "/send" => {
                    writeln!(w, "\x1b[34myou\x1b[0m: {rest}")?;
                    ChatEvent::builder().new_message(rest).sign(key)
                }
                "/name" => ChatEvent::builder().set_name(rest).sign(key),
                "/join" => ChatEvent::builder().node_joined().sign(key),
                "/leave" => ChatEvent::builder().node_left().sign(key),
                _ => {
                    writeln!(w, "unknown action {action}")?;
                    continue;
                }
            }
        } else {
            writeln!(w, "\x1b[34myou\x1b[0m: {line}")?;
            ChatEvent::builder().new_message(line).sign(key)
        };

        sender.broadcast(event.to_vec().into()).await?;
    }

    router.shutdown().await?;

    Ok(())
}

async fn subscribe_loop(mut receiver: GossipReceiver, mut stdout: SharedWriter) -> Result<()> {
    let mut names = HashMap::new();
    while let Some(gossip_event) = receiver.try_next().await? {
        if let Event::Received(gossip_message) = gossip_event {
            let unverified_event =
                postcard::from_bytes::<SignedChatEvent>(&gossip_message.content)?;
            let Ok(event) = unverified_event.verify_into() else {
                continue;
            };
            match event {
                ChatEvent::NewMessage { author, message } => {
                    let name = names.get(&author).map_or_else(
                        || author.fmt_short(),
                        |name| format!("{} \"{name}\"", author.fmt_short()),
                    );

                    writeln!(stdout, "\x1b[31m{}\x1b[0m: {}", name, message)?;
                }
                ChatEvent::SetName { author, name } => {
                    let prev_name = names.get(&author).map_or_else(
                        || author.fmt_short(),
                        |name| format!("{} \"{name}\"", author.fmt_short()),
                    );

                    names.insert(author, name.clone());

                    writeln!(
                        stdout,
                        "{prev_name} is now known as {} \"{name}\"",
                        author.fmt_short()
                    )?;
                }
                ChatEvent::NodeJoined { author } => {
                    let name = names.get(&author).map_or_else(
                        || author.fmt_short(),
                        |name| format!("{} \"{name}\"", author.fmt_short()),
                    );

                    writeln!(stdout, "{} joined", name)?;
                }
                ChatEvent::NodeLeft { author } => {
                    let name = names.get(&author).map_or_else(
                        || author.fmt_short(),
                        |name| format!("{} \"{name}\"", author.fmt_short()),
                    );

                    writeln!(stdout, "{} left", name)?;
                }
            }
        }
    }
    Ok(())
}
