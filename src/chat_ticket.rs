use std::fmt;
use std::str::FromStr;

use iroh::NodeAddr;
use iroh_base::ticket::{ParseError, Ticket};
use iroh_gossip::proto::TopicId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct ChatTicket {
    pub topic: TopicId,
    pub nodes: Vec<NodeAddr>,
}

impl Ticket for ChatTicket {
    const KIND: &'static str = "msg/0";

    fn to_bytes(&self) -> Vec<u8> {
        postcard::to_allocvec(self).unwrap()
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self, ParseError> {
        postcard::from_bytes(bytes).map_err(ParseError::from)
    }
}

impl FromStr for ChatTicket {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        <Self as Ticket>::deserialize(s)
    }
}

impl fmt::Display for ChatTicket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", <Self as Ticket>::serialize(self))
    }
}
