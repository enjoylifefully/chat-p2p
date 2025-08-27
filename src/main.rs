#![allow(unused_variables)]

mod chat_event;
mod chat_ticket;
mod config;

use std::io::Write;
use std::str::FromStr;

use anyhow::Result;
use clap::Parser;
use futures_lite::StreamExt;
use iroh::protocol::Router;
use iroh::{Endpoint, NodeAddr, NodeId, Watcher};
use iroh_gossip::api::{Event, GossipReceiver};
use iroh_gossip::net::Gossip;
use iroh_gossip::proto::TopicId;
use owo_colors::OwoColorize;
use rustyline_async::{Readline, ReadlineEvent, SharedWriter};
use {base58, dirs, postcard, thiserror};

use crate::chat_event::{ChatEvent, ChatEventBuilder, SignedChatEvent, actor_rbg};
use crate::chat_ticket::ChatTicket;
use crate::config::{add_friends, generate_secret_key, load_friends_without_me};

/// Chat over iroh-gossip
///
/// This broadcasts unsigned messages over iroh-gossip.
///
/// By default a new node id is created when starting the example.
///
/// By default, we use the default n0 discovery services to dial by `NodeId`.
#[derive(Parser, Debug)]
struct Args {
    name: String,

    #[clap(subcommand)]
    command: Command,
}

#[derive(Parser, Debug)]
enum Command {
    Add {
        #[arg(required = true)]
        friends: Vec<String>,
    },
    /// Open a chat room for a topic and print a ticket for others to join.
    Join {
        /// The ticket, as base32 string.
        topic: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let (topic_id, topic) = match &args.command {
        Command::Add { friends } => {
            add_friends(friends)?;
            return Ok(());
        }
        Command::Join { topic } => {
            let hash = blake3::hash(topic.as_bytes());
            let topic_id = TopicId::from_bytes(*hash.as_bytes());
            (topic_id, topic)
        }
    };

    let secret_key = generate_secret_key(&args.name)?;
    let public_key = secret_key.public();

    let mut friends = load_friends_without_me(public_key)?;

    let endpoint = Endpoint::builder()
        .secret_key(secret_key)
        .discovery_n0()
        .bind()
        .await?;
    // writeln!(w, "| :   {}", endpoint.secret_key().public())?;

    let gossip = Gossip::builder().spawn(endpoint.clone());

    let router = Router::builder(endpoint.clone())
        .accept(iroh_gossip::ALPN, gossip.clone())
        .spawn();

    let (mut rl, mut stdout) = Readline::new("> ".to_string())?;
    rl.should_print_line_on(false, false);
    rl.clear()?;

    writeln!(stdout, "{}", topic)?;
    writeln!(stdout, "{}", base58::encode(public_key).into_string())?;

    let (sender, receiver) = gossip.subscribe(topic_id, friends).await?.split();

    tokio::spawn(subscribe_loop(receiver, stdout.clone()));

    let key = endpoint.secret_key().secret();
    let mut name = args.name;

    while let Ok(line_event) = rl.readline().await {
        let ReadlineEvent::Line(line) = line_event else {
            break;
        };
        let line = line.trim();

        let event = if line.starts_with("/") {
            let (action, rest) = match line.split_once(char::is_whitespace) {
                Some((cmd, rest)) => (cmd, rest.trim()),
                None => (line, ""),
            };

            match action {
                "/send" => {
                    if rest.is_empty() {
                        continue;
                    }

                    let message_event = ChatEvent::NewMessage {
                        actor: endpoint.node_id(),
                        name: name.clone(),
                        message: line.to_string(),
                    };

                    writeln!(stdout, "{}", message_event.bold())?;

                    ChatEvent::builder().new_message(&name, rest).sign(key)
                }
                "/name" => {
                    name = rest.to_string();
                    continue;
                }
                //ChatEvent::builder().set_name(rest).sign(key),
                "/join" => ChatEvent::builder().node_joined().sign(key),
                "/leave" => ChatEvent::builder().node_left().sign(key),
                _ => {
                    writeln!(stdout, "unknown action {action}")?;

                    continue;
                }
            }
        } else {
            if line.is_empty() {
                continue;
            }

            let message_event = ChatEvent::NewMessage {
                actor: endpoint.node_id(),
                name: name.clone(),
                message: line.to_string(),
            };

            writeln!(stdout, "{}", message_event.bold())?;

            ChatEvent::builder().new_message(&name, line).sign(key)
        };

        sender.broadcast(event.to_vec().into()).await?;
    }

    router.shutdown().await?;

    Ok(())
}

async fn subscribe_loop(mut receiver: GossipReceiver, mut stdout: SharedWriter) -> Result<()> {
    while let Some(gossip_event) = receiver.try_next().await? {
        if let Event::Received(gossip_message) = gossip_event {
            let unverified_event =
                postcard::from_bytes::<SignedChatEvent>(&gossip_message.content)?;
            let Ok(event) = unverified_event.verify_into() else {
                continue;
            };
            match &event {
                ChatEvent::NewMessage {
                    actor,
                    name,
                    message,
                } => {
                    writeln!(stdout, "{event}")?;
                }
                ChatEvent::SetName { actor, name } => {
                    todo!();
                    // let prev_name = names.get(&actor).map_or_else(
                    //     || actor.fmt_short(),
                    //     |name| format!("{} \"{name}\"", actor.fmt_short()),
                    // );

                    // writeln!(
                    //     stdout,
                    //     "{prev_name} is now known as {} \"{name}\"",
                    //     actor.fmt_short()
                    // )?;
                }
                ChatEvent::NodeJoined { actor } => {
                    writeln!(stdout, "{event}")?;
                }
                ChatEvent::NodeLeft { actor } => {
                    writeln!(stdout, "{event}")?;
                }
            }
        }
    }
    Ok(())
}
