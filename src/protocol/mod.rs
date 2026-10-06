pub mod apps;
pub mod auth;
pub mod battery;
pub mod chunk;
pub mod music;
pub mod watchface;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("packet is truncated")]
    Truncated,

    #[error("unexpected first byte {0:#04x}, expected 0x03 or 0x04")]
    BadMagic(u8),

    #[error("continuation chunk arrived with no message in progress")]
    UnexpectedContinuation,

    #[error("chunk handle {actual} does not match the message in progress ({expected})")]
    HandleMismatch { expected: u8, actual: u8 },

    #[error("received an encrypted message before the session key was set")]
    NoSessionKey,

    #[error("aes input length {0} is not a multiple of 16")]
    BadBlockLength(usize),

    #[error("payload is empty")]
    EmptyPayload,

    #[error("mtu {0} is too small to carry a chunk")]
    MtuTooSmall(u16),

    #[error("auth response is malformed")]
    MalformedAuthResponse,

    #[error("the band rejected the auth key")]
    AuthRejected,

    #[error("b163 key generation failed")]
    KeyGenerationFailed,
}
