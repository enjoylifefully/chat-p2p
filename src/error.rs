pub use std::io::Error as IoError;

pub use base58::decode::Error as DecodeError;
pub use ed25519_dalek::SignatureError as DalekError;
pub use postcard::Error as PostcardError;
pub use thiserror::Error as ThisError;

#[derive(Debug, ThisError)]
#[error("{self:?}")]
pub enum ConfigError {
    NoHomeDir,

    // externals
    Base58(#[from] DecodeError),
    Io(#[from] IoError),
}

#[derive(Debug, ThisError)]
#[error("{self:?}")]
pub enum DiscoveryError {
    InvalidNonce,
    Timeout,

    // externals
    Io(#[from] IoError),
    Base58(#[from] DecodeError),
    Dalek(#[from] DalekError),
    Postcard(#[from] PostcardError),
}

#[derive(Debug, ThisError)]
#[error("{self:?}")]
pub enum SignatureError {
    // externals
    Dalek(#[from] DalekError),
    Postcard(#[from] PostcardError),
}
