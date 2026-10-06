use crate::config::AuthKey;
use crate::crypto::b163::{KeyPair, PUBLIC_KEY_LEN};
use crate::crypto::{aes_ecb_encrypt_block, derive_sequence_number, derive_session_key};
use crate::protocol::ProtocolError;

/// Zepp 2021 authentication endpoint.
pub const ENDPOINT_AUTH: u16 = 0x0082;

const RESPONSE: u8 = 0x10;
const STEP_KEY_EXCHANGE: u8 = 0x04;
const STEP_CHALLENGE: u8 = 0x05;
const SUCCESS: u8 = 0x01;
const REJECTED: u8 = 0x25;

/// What the band and we agreed on for this connection.
#[derive(Clone, Copy)]
pub struct SessionParams {
    pub session_key: [u8; 16],
    pub sequence: u32,
}

impl std::fmt::Debug for SessionParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SessionParams(session_key: redacted)")
    }
}

pub enum AuthStep {
    SendChallenge {
        payload: Vec<u8>,
        params: SessionParams,
    },
    Authenticated,
    Ignored,
}

impl std::fmt::Debug for AuthStep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthStep::SendChallenge { .. } => f.write_str("SendChallenge(payload: redacted)"),
            AuthStep::Authenticated => f.write_str("Authenticated"),
            AuthStep::Ignored => f.write_str("Ignored"),
        }
    }
}

pub struct Authenticator {
    auth_key: AuthKey,
    keypair: KeyPair,
}

impl Authenticator {
    pub fn new(auth_key: AuthKey, keypair: KeyPair) -> Self {
        Self { auth_key, keypair }
    }

    /// First message of the handshake: command header plus our public key.
    pub fn public_key_message(&self) -> Vec<u8> {
        let mut msg = Vec::with_capacity(4 + PUBLIC_KEY_LEN);
        msg.extend_from_slice(&[0x04, 0x02, 0x00, 0x02]);
        msg.extend_from_slice(self.keypair.public());
        msg
    }

    pub fn handle(&mut self, payload: &[u8]) -> Result<AuthStep, ProtocolError> {
        match payload {
            [RESPONSE, STEP_KEY_EXCHANGE, SUCCESS, rest @ ..] => self.on_key_exchange(rest),
            [RESPONSE, STEP_CHALLENGE, SUCCESS, ..] => Ok(AuthStep::Authenticated),
            // REJECTED is the answer to a wrong key. Any other answer to the
            // challenge refuses us as well, and says so now: left unread, it
            // would cost the wait for a reply that is not coming, and read as
            // a band that did not answer rather than one that said no.
            [RESPONSE, STEP_CHALLENGE, status, ..] => {
                if *status != REJECTED {
                    tracing::debug!("the band refused the challenge with status {status:#04x}");
                }
                Err(ProtocolError::AuthRejected)
            }
            _ => Ok(AuthStep::Ignored),
        }
    }

    fn on_key_exchange(&mut self, rest: &[u8]) -> Result<AuthStep, ProtocolError> {
        if rest.len() < 16 + PUBLIC_KEY_LEN {
            return Err(ProtocolError::MalformedAuthResponse);
        }
        let remote_random = <[u8; 16]>::try_from(&rest[0..16])
            .map_err(|_| ProtocolError::MalformedAuthResponse)?;
        let remote_public = <[u8; PUBLIC_KEY_LEN]>::try_from(&rest[16..16 + PUBLIC_KEY_LEN])
            .map_err(|_| ProtocolError::MalformedAuthResponse)?;

        let shared = self.keypair.shared_secret(&remote_public)?;
        let sequence = derive_sequence_number(&shared)?;
        let session_key = derive_session_key(&shared, self.auth_key.bytes())?;

        let under_auth_key = aes_ecb_encrypt_block(self.auth_key.bytes(), &remote_random);
        let under_session_key = aes_ecb_encrypt_block(&session_key, &remote_random);

        let mut payload = Vec::with_capacity(33);
        payload.push(STEP_CHALLENGE);
        payload.extend_from_slice(&under_auth_key);
        payload.extend_from_slice(&under_session_key);

        Ok(AuthStep::SendChallenge {
            payload,
            params: SessionParams {
                session_key,
                sequence,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AuthKey;
    use crate::crypto::b163::KeyPair;

    const GB_PRIVATE: &str = "0b42b9e61c23340e35c16e2e7de433f4b5859a72ec114027";
    const GB_REMOTE_PUBLIC: &str = "e6016aba1de7ac0f0c7f0ff7e2243e6662b5e03b01000000ad8a4bedc76a1efde7725cc662b548355\
13e3d5705000000";
    const REMOTE_RANDOM: &str = "0102030405060708090a0b0c0d0e0f10";
    const TEST_AUTH_KEY: &str = "000102030405060708090a0b0c0d0e0f";

    fn fixed_authenticator() -> Authenticator {
        let private = <[u8; 24]>::try_from(hex::decode(GB_PRIVATE).unwrap().as_slice()).unwrap();
        Authenticator::new(
            AuthKey::from_hex(TEST_AUTH_KEY).unwrap(),
            KeyPair::from_private(private).unwrap(),
        )
    }

    /// 0x10 RESPONSE, 0x04 step, 0x01 SUCCESS, 16-byte random, 48-byte public key.
    fn band_key_response() -> Vec<u8> {
        let mut payload = vec![0x10, 0x04, 0x01];
        payload.extend_from_slice(&hex::decode(REMOTE_RANDOM).unwrap());
        payload.extend_from_slice(&hex::decode(GB_REMOTE_PUBLIC).unwrap());
        assert_eq!(payload.len(), 67);
        payload
    }

    #[test]
    fn builds_the_public_key_message() {
        let auth = fixed_authenticator();
        let msg = auth.public_key_message();
        assert_eq!(msg.len(), 52);
        assert_eq!(&msg[0..4], &[0x04, 0x02, 0x00, 0x02]);
        assert_eq!(
            hex::encode(&msg[4..]),
            "a7bf78d434e3bd908a1f16c50fcf87307d814dc005000000d8f4a2385f110e16062d9da27f5f1c3038abb56404000000"
        );
    }

    #[test]
    fn derives_the_session_parameters_and_challenge() {
        let mut auth = fixed_authenticator();
        let step = auth.handle(&band_key_response()).unwrap();
        let (payload, params) = match step {
            AuthStep::SendChallenge { payload, params } => (payload, params),
            other => panic!("expected a challenge, got {other:?}"),
        };

        assert_eq!(params.sequence, 0x4652_9911);
        assert_eq!(hex::encode(params.session_key), "ce58de022ef27c075827f1550a0d0e0f");

        assert_eq!(payload.len(), 33);
        assert_eq!(payload[0], 0x05);
        // AES(remote_random) under the auth key, then under the session key.
        assert_eq!(hex::encode(&payload[1..17]), "0892085605be8f349f584af993df11f8");
        assert_eq!(hex::encode(&payload[17..33]), "471c71f98f2bf81ed21ef42f411c7b7d");
    }

    #[test]
    fn reports_success() {
        let mut auth = fixed_authenticator();
        auth.handle(&band_key_response()).unwrap();
        assert!(matches!(
            auth.handle(&[0x10, 0x05, 0x01]).unwrap(),
            AuthStep::Authenticated
        ));
    }

    #[test]
    fn reports_a_rejected_key() {
        let mut auth = fixed_authenticator();
        auth.handle(&band_key_response()).unwrap();
        let err = auth.handle(&[0x10, 0x05, 0x25]).unwrap_err();
        assert!(matches!(err, ProtocolError::AuthRejected));
    }

    /// Any status but success, answering the challenge, is a refusal.
    #[test]
    fn any_other_answer_to_the_challenge_is_a_refusal() {
        for status in [0x02, 0x03, 0x24, 0x26, 0xff] {
            let mut auth = fixed_authenticator();
            auth.handle(&band_key_response()).unwrap();
            let err = auth.handle(&[0x10, 0x05, status]).unwrap_err();
            assert!(matches!(err, ProtocolError::AuthRejected), "status {status:#04x}");
        }
    }

    #[test]
    fn ignores_unrelated_payloads() {
        let mut auth = fixed_authenticator();
        assert!(matches!(auth.handle(&[0x10, 0x09, 0x01]).unwrap(), AuthStep::Ignored));
    }

    #[test]
    fn rejects_a_short_key_response() {
        let mut auth = fixed_authenticator();
        let short = vec![0x10, 0x04, 0x01, 0x00, 0x00];
        assert!(matches!(
            auth.handle(&short).unwrap_err(),
            ProtocolError::MalformedAuthResponse
        ));
    }
}
