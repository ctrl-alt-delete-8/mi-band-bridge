use crate::protocol::ProtocolError;

pub const PRIVATE_KEY_LEN: usize = 24;
pub const PUBLIC_KEY_LEN: usize = 48;
pub const SHARED_SECRET_LEN: usize = 48;

unsafe extern "C" {
    /// Returns 1 on success, 0 when the scalar is too small.
    /// Note: this call rewrites `private_key` in place.
    fn ecdh_generate_keys(public_key: *mut u8, private_key: *mut u8) -> i32;

    fn ecdh_shared_secret(private_key: *const u8, others_pub: *const u8, output: *mut u8) -> i32;
}

/// An ephemeral B-163 key pair for one connection.
pub struct KeyPair {
    private: [u8; PRIVATE_KEY_LEN],
    public: [u8; PUBLIC_KEY_LEN],
}

impl KeyPair {
    /// Build a pair from a caller-supplied scalar.
    ///
    /// `ecdh_generate_keys` clears the bits above the curve order, so the
    /// stored private key is the masked value, not the input. Using the
    /// unmasked input for the shared secret would produce the wrong answer.
    pub fn from_private(private: [u8; PRIVATE_KEY_LEN]) -> Result<Self, ProtocolError> {
        let mut private = private;
        let mut public = [0u8; PUBLIC_KEY_LEN];
        let ok = unsafe { ecdh_generate_keys(public.as_mut_ptr(), private.as_mut_ptr()) };
        if ok != 1 {
            return Err(ProtocolError::KeyGenerationFailed);
        }
        Ok(Self { private, public })
    }

    /// Draw a fresh scalar. Retries the rare case where the scalar is too small.
    pub fn generate() -> Result<Self, ProtocolError> {
        for _ in 0..16 {
            let mut scalar = [0u8; PRIVATE_KEY_LEN];
            rand::fill(&mut scalar);
            if let Ok(pair) = Self::from_private(scalar) {
                return Ok(pair);
            }
        }
        Err(ProtocolError::KeyGenerationFailed)
    }

    pub fn public(&self) -> &[u8; PUBLIC_KEY_LEN] {
        &self.public
    }

    pub fn shared_secret(
        &self,
        remote_public: &[u8; PUBLIC_KEY_LEN],
    ) -> Result<[u8; SHARED_SECRET_LEN], ProtocolError> {
        let mut out = [0u8; SHARED_SECRET_LEN];
        let ok = unsafe {
            ecdh_shared_secret(self.private.as_ptr(), remote_public.as_ptr(), out.as_mut_ptr())
        };
        if ok != 1 {
            return Err(ProtocolError::KeyGenerationFailed);
        }
        Ok(out)
    }

    #[cfg(test)]
    pub fn private_for_test(&self) -> &[u8; PRIVATE_KEY_LEN] {
        &self.private
    }
}

impl std::fmt::Debug for KeyPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("KeyPair(private: redacted)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The private key from the Gadgetbridge InitOperation2021 self-test.
    const GB_PRIVATE: &str = "0b42b9e61c23340e35c16e2e7de433f4b5859a72ec114027";
    /// The remote public key from that same self-test.
    const GB_REMOTE_PUBLIC: &str = "e6016aba1de7ac0f0c7f0ff7e2243e6662b5e03b01000000ad8a4bedc76a1efde7725cc662b548355\
13e3d5705000000";

    fn arr24(hexed: &str) -> [u8; 24] {
        let v = hex::decode(hexed).unwrap();
        <[u8; 24]>::try_from(v.as_slice()).unwrap()
    }
    fn arr48(hexed: &str) -> [u8; 48] {
        let v = hex::decode(hexed).unwrap();
        <[u8; 48]>::try_from(v.as_slice()).unwrap()
    }

    #[test]
    fn key_sizes_match_b163() {
        assert_eq!(PRIVATE_KEY_LEN, 24);
        assert_eq!(PUBLIC_KEY_LEN, 48);
        assert_eq!(SHARED_SECRET_LEN, 48);
    }

    #[test]
    fn generation_masks_the_private_key_and_keeps_the_masked_value() {
        // ecdh_generate_keys clears the bits above the curve order.
        let pair = KeyPair::from_private(arr24(GB_PRIVATE)).unwrap();
        assert_eq!(
            hex::encode(pair.private_for_test()),
            "0b42b9e61c23340e35c16e2e7de433f4b5859a7200000000"
        );
        assert_eq!(
            hex::encode(pair.public()),
            "a7bf78d434e3bd908a1f16c50fcf87307d814dc005000000d8f4a2385f110e16062d9da27f5f1c3038abb56404000000"
        );
    }

    #[test]
    fn reproduces_the_gadgetbridge_shared_secret() {
        let pair = KeyPair::from_private(arr24(GB_PRIVATE)).unwrap();
        let shared = pair.shared_secret(&arr48(GB_REMOTE_PUBLIC)).unwrap();
        assert_eq!(
            hex::encode(shared),
            "119952464b0ff4f0ce59dc012af77a00502efb5e060000003479ea6b37fa4edb7fdaf4c0c2df3ea5a637f4ac06000000"
        );
    }

    #[test]
    fn two_parties_agree_on_the_same_secret() {
        let alice = KeyPair::generate().unwrap();
        let bob = KeyPair::generate().unwrap();
        let a = alice.shared_secret(bob.public()).unwrap();
        let b = bob.shared_secret(alice.public()).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn generate_produces_distinct_keys() {
        let a = KeyPair::generate().unwrap();
        let b = KeyPair::generate().unwrap();
        assert_ne!(a.public(), b.public());
    }

    #[test]
    fn rejects_an_all_zero_private_key() {
        assert!(KeyPair::from_private([0u8; 24]).is_err());
    }

    #[test]
    fn debug_redacts_the_private_key() {
        let pair = KeyPair::from_private(arr24(GB_PRIVATE)).unwrap();
        let shown = format!("{pair:?}");
        assert!(!shown.contains("0b42b9"), "debug leaked the key: {shown}");
        assert!(shown.contains("redacted"));
    }
}
