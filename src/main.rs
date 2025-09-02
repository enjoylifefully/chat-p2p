#![allow(unused_variables)]

mod chat_event;
mod config;

use std::io::Write;

use anyhow::Result;
use clap::Parser;
use clap::builder::Styles;
use futures_lite::StreamExt;
use iroh::Endpoint;
use iroh::protocol::Router;
use iroh_gossip::api::{Event, GossipReceiver};
use iroh_gossip::net::Gossip;
use iroh_gossip::proto::TopicId;
use owo_colors::OwoColorize;
use rustyline_async::{Readline, ReadlineEvent, SharedWriter};
use {base58, postcard};

use crate::chat_event::{ChatEvent, SignedChatEvent};
use crate::config::{add_friends, generate_secret_key, load_friends_without_me};

#[derive(Parser, Debug)]
#[clap(styles = Styles::plain())]
struct Args {
    /// The topic name.
    topic: String,

    /// Your name.
    #[clap(short = 'n', long)]
    name: Option<String>,

    /// Your seed.
    #[clap(short = 's', long)]
    seed: Option<String>,

    /// Friends to add.
    #[clap(short = 'f', long, num_args = 1..)]
    friends: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    add_friends(&args.friends)?;
    let topic = args.topic;
    let hash = blake3::hash(topic.as_bytes());
    let topic_id = TopicId::from_bytes(*hash.as_bytes());

    let secret_key = generate_secret_key(args.seed.as_deref().unwrap_or(""))?;
    let public_key = secret_key.public();

    let friends = load_friends_without_me(public_key)?;

    let endpoint = Endpoint::builder()
        .secret_key(secret_key)
        .discovery_n0()
        .bind()
        .await?;

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
    let mut name = args.name.unwrap_or_default();

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
                        message: rest.to_string(),
                    };

                    writeln!(stdout, "{}", message_event.bold())?;

                    rl.add_history_entry(rest.to_string());

                    ChatEvent::builder().new_message(&name, rest).sign(key)
                }
                "/name" => {
                    // writeln!(stdout, "{name} -> {rest}")?;

                    name = rest.to_string();

                    continue;
                }
                "/join" => ChatEvent::builder().node_joined().sign(key),
                "/leave" => ChatEvent::builder().node_left().sign(key),
                "/exit" => break,
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

            rl.add_history_entry(line.to_string());

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
