use std::fmt;

use ed25519_dalek::ed25519::signature::Signer;
use ed25519_dalek::{Signature, SignatureError as DalekError, SigningKey, VerifyingKey};
use iroh::NodeId;
use owo_colors::{FgDynColorDisplay, OwoColorize, Rgb};
use palette::{FromColor as _, Hsl, Srgb};
use postcard::Error as PostcardError;
use serde::{Deserialize, Serialize};
use thiserror::Error as ThisError;

type Nonce = [u8; 16];

// region:       --- structs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChatEvent {
    NewMessage {
        actor: NodeId,
        name: String,
        message: String,
    },
    SetName {
        actor: NodeId,
        name: String,
    },
    NodeJoined {
        actor: NodeId,
    },
    NodeLeft {
        actor: NodeId,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChatEventBody {
    NewMessage { name: String, message: String },
    SetName { name: String },
    NodeJoined,
    NodeLeft,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedChatEvent {
    // version: u32,
    body_bytes: Vec<u8>,
    nonce: Nonce,
    key: VerifyingKey,
    sig: Signature,
}

// endregion:    --- structs

// region:       --- ChatEvent impl

impl ChatEvent {
    pub fn builder() -> ChatEventBuilder<Initial, Initial> {
        ChatEventBuilder::new()
    }

    pub fn actor(&self) -> NodeId {
        *match self {
            Self::NewMessage { actor, .. } => actor,
            Self::SetName { actor, .. } => actor,
            Self::NodeLeft { actor, .. } => actor,
            Self::NodeJoined { actor, .. } => actor,
        }
    }
}

pub fn actor_rbg(actor: &NodeId) -> (u8, u8, u8) {
    let bytes = actor.as_bytes();
    let hue = (u16::from_be_bytes([bytes[0], bytes[1]]) % 360) as f32;
    let hsl = Hsl::new(hue, 0.65, 0.55);

    Srgb::from_color(hsl).into_format().into_components()
}

impl fmt::Display for ChatEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NewMessage {
                actor,
                name,
                message,
            } => {
                let (r, g, b) = actor_rbg(actor);
                let name = name.trim();
                if name.is_empty() {
                    write!(f, "{} {message}", actor.fmt_short().truecolor(r, g, b),)
                } else {
                    write!(
                        f,
                        "{} {} {message}",
                        actor.fmt_short().truecolor(r, g, b),
                        name.truecolor(r, g, b)
                    )
                }
            }
            Self::SetName { actor, name } => write!(f, ""),
            Self::NodeLeft { actor, .. } => write!(f, ""),
            Self::NodeJoined { actor, .. } => write!(f, ""),
        }
    }
}

// endregion:    --- ChatEvent impl

// region:       --- SignedChatEvent impl

impl SignedChatEvent {
    pub fn verify_into(self) -> Result<ChatEvent, SignatureError> {
        let body_bytes_len = self.body_bytes.len();
        let mut with_nonce = self.body_bytes;

        with_nonce.extend_from_slice(&self.nonce);

        self.key.verify_strict(&with_nonce, &self.sig)?;

        let body_bytes = &with_nonce[..body_bytes_len];
        let event_body = postcard::from_bytes(body_bytes)?;
        let actor = NodeId::from(self.key);

        let event = match event_body {
            ChatEventBody::NewMessage { name, message } => ChatEvent::NewMessage {
                actor,
                name,
                message,
            },
            ChatEventBody::SetName { name } => ChatEvent::SetName { actor, name },
            ChatEventBody::NodeJoined => ChatEvent::NodeJoined { actor },
            ChatEventBody::NodeLeft => ChatEvent::NodeLeft { actor },
        };

        Ok(event)
    }

    pub fn to_vec(&self) -> Vec<u8> {
        postcard::to_allocvec(self).unwrap()
    }
}

// endregion:    --- SignedChatEvent impl

// Initial state
pub struct Initial;

// region:       --- EventState

trait EventState {}

pub struct NewMessage {
    name: String,
    message: String,
}

pub struct SetName {
    name: String,
}

pub struct NodeJoined;

pub struct NodeLeft;

impl EventState for Initial {}

impl EventState for NewMessage {}

impl EventState for SetName {}

impl EventState for NodeJoined {}

impl EventState for NodeLeft {}

// endregion:    --- EventState

// region:       --- SignState

trait SignState {}

pub struct ReadyToSign;

pub struct Signed {
    key: VerifyingKey,
}

impl SignState for Initial {}

impl SignState for ReadyToSign {}

impl SignState for Signed {}

// endregion:    --- SignState

// TODO: fazer poder usar o sign antes também

pub struct ChatEventBuilder<E: EventState, S: SignState> {
    sign: S,
    event: E,
}

// region:       --- impl ChatEventBuilder

impl ChatEventBuilder<Initial, Initial> {
    pub fn new() -> Self {
        Self {
            sign: Initial,
            event: Initial,
        }
    }

    pub fn new_message(
        self,
        name: impl Into<String>,
        message: impl Into<String>,
    ) -> ChatEventBuilder<NewMessage, ReadyToSign> {
        ChatEventBuilder {
            sign: ReadyToSign,
            event: NewMessage {
                name: name.into(),
                message: message.into(),
            },
        }
    }

    pub fn set_name(self, name: impl Into<String>) -> ChatEventBuilder<SetName, ReadyToSign> {
        ChatEventBuilder {
            sign: ReadyToSign,
            event: SetName { name: name.into() },
        }
    }

    pub fn node_joined(self) -> ChatEventBuilder<NodeJoined, ReadyToSign> {
        ChatEventBuilder {
            sign: ReadyToSign,
            event: NodeJoined,
        }
    }

    pub fn node_left(self) -> ChatEventBuilder<NodeLeft, ReadyToSign> {
        ChatEventBuilder {
            sign: ReadyToSign,
            event: NodeLeft,
        }
    }
}

impl ChatEventBuilder<NewMessage, ReadyToSign> {
    pub fn sign(self, key: &SigningKey) -> SignedChatEvent {
        let body = ChatEventBody::NewMessage {
            name: self.event.name,
            message: self.event.message,
        };

        sign_chat_event(body, key)
    }
}

impl ChatEventBuilder<SetName, ReadyToSign> {
    pub fn sign(self, key: &SigningKey) -> SignedChatEvent {
        let body = ChatEventBody::SetName {
            name: self.event.name,
        };

        sign_chat_event(body, key)
    }
}

impl ChatEventBuilder<NodeJoined, ReadyToSign> {
    pub fn sign(self, key: &SigningKey) -> SignedChatEvent {
        let body = ChatEventBody::NodeJoined;

        sign_chat_event(body, key)
    }
}

impl ChatEventBuilder<NodeLeft, ReadyToSign> {
    pub fn sign(self, key: &SigningKey) -> SignedChatEvent {
        let body = ChatEventBody::NodeLeft;

        sign_chat_event(body, key)
    }
}

// endregion:    --- impl ChatEventBuilder

// region:       --- utils

#[derive(Debug, ThisError)]
#[error("{self:?}")]
pub enum SignatureError {
    Dalek(#[from] DalekError),
    Postcard(#[from] PostcardError),
}

fn sign_chat_event(event: ChatEventBody, key: &SigningKey) -> SignedChatEvent {
    let mut bytes = postcard::to_allocvec(&event).unwrap();
    let body_bytes_len = bytes.len();
    let nonce = rand::random::<Nonce>();

    bytes.extend_from_slice(&nonce);

    let sig = key.sign(&bytes);

    bytes.truncate(body_bytes_len);

    SignedChatEvent {
        body_bytes: bytes,
        nonce,
        key: key.verifying_key(),
        sig,
    }
}

// endregion:    --- utils
