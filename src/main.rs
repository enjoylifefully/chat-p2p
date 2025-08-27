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

    // parse the cli command
    let (topic, nodes) = match &args.command {
        Command::Open => {
            let topic = TopicId::from_bytes(rand::random());
            println!("> opening chat room for topic {topic}");
            (topic, vec![])
        }
        Command::Join { ticket } => {
            let ChatTicket { topic, nodes } = ChatTicket::from_str(ticket)?;
            println!("| TOPIC:  {topic}");
            (topic, nodes)
        }
    };

    let endpoint = Endpoint::builder().discovery_n0().bind().await?;

    println!("| AUTH:   {}", endpoint.secret_key().public());

    let gossip = Gossip::builder().spawn(endpoint.clone());

    let router = Router::builder(endpoint.clone())
        .accept(iroh_gossip::ALPN, gossip.clone())
        .spawn();

    let ticket = {
        let me = endpoint.node_addr().get().unwrap();
        let nodes = vec![me];
        ChatTicket { topic, nodes }
    };

    println!("| TICKET: {ticket}");

    let node_ids = nodes.iter().map(|p| p.node_id).collect();
    for node in nodes.into_iter() {
        endpoint.add_node_addr(node)?;
    }
    let (sender, receiver) = gossip.subscribe_and_join(topic, node_ids).await?.split();

    println!("| CONNECTED");

    tokio::spawn(subscribe_loop(receiver));

    let (line_sender, mut line_receiver) = tokio::sync::mpsc::channel(1);

    std::thread::spawn(move || input_loop(line_sender));

    let key = endpoint.secret_key().secret();

    while let Some(line) = line_receiver.recv().await {
        let (action, rest) = match line.split_once(char::is_whitespace) {
            Some((cmd, rest)) => (cmd, rest.trim_start()),
            None => (line.as_str(), ""),
        };

        let event = match action {
            "send" => ChatEvent::builder().new_message(rest).sign(key),
            "name" => ChatEvent::builder().set_name(rest).sign(key),
            "join" => ChatEvent::builder().node_joined().sign(key),
            "leave" => ChatEvent::builder().node_left().sign(key),
            _ => {
                println!("| UNKNOWN ACTION: {action}");
                continue;
            }
        };

        sender.broadcast(event.to_vec().into()).await?;

        print!("\x1B[2K\r");
        print!("\x1B[2K\r");
        print!("\x1B[2K\r");
        stdout().flush();
        println!("\x1b[34myou\x1b[0m: {rest}");
    }
    router.shutdown().await?;

    Ok(())
}

// Handle incoming events
async fn subscribe_loop(mut receiver: GossipReceiver) -> Result<()> {
    // keep track of the mapping between `NodeId`s and names
    let mut names = HashMap::new();
    // iterate over all events
    dbg!(1);
    while let Some(gossip_event) = receiver.try_next().await? {
        // if the Event is a `GossipEvent::Received`, let's deserialize the message:
        //
        dbg!(1);
        if let Event::Received(gossip_message) = gossip_event {
            let unverified_event =
                postcard::from_bytes::<SignedChatEvent>(&gossip_message.content)?;
            let Ok(event) = unverified_event.verify_into() else {
                continue;
            };
            dbg!(&event);
            match event {
                ChatEvent::NewMessage { author, message } => {
                    // if it's a `Message` message,
                    // get the name from the map
                    // and print the message
                    let name = names
                        .get(&author)
                        .map_or_else(|| author.fmt_short(), String::to_string);
                    println!("\x1b[31m{}\x1b[0m: {}", name, message);
                }
                ChatEvent::SetName { author, name } => {
                    // and print the name
                    names.insert(author, name.clone());
                    println!("> {} is now known as {}", author.fmt_short(), name);
                }
                ChatEvent::NodeJoined { author } => {
                    let name = names
                        .get(&author)
                        .map_or_else(|| author.fmt_short(), String::to_string);
                    println!("{} joined", name);
                }
                ChatEvent::NodeLeft { author } => {
                    let name = names
                        .get(&author)
                        .map_or_else(|| author.fmt_short(), String::to_string);
                    println!("{} left", name);
                }
            }
        }
    }
    Ok(())
}

fn input_loop(line_tx: tokio::sync::mpsc::Sender<String>) -> Result<()> {
    let mut buffer = String::new();
    let stdin = std::io::stdin(); // We get `Stdin` here.
    loop {
        stdin.read_line(&mut buffer)?;
        line_tx.blocking_send(buffer.clone())?;
        buffer.clear();
    }
}
