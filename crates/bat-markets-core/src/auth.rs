use core::fmt;
use std::env;

use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::error::{ErrorKind, MarketError, Result};

type HmacSha256 = Hmac<Sha256>;

/// Signer abstraction for private request authentication.
pub trait Signer: Send + Sync {
    fn sign_hex(&self, payload: &[u8]) -> Result<String>;
}

/// In-memory signer used by tests or controlled environments.
pub struct MemorySigner {
    secret: Vec<u8>,
}

impl MemorySigner {
    #[must_use]
    pub fn new(secret: impl AsRef<[u8]>) -> Self {
        Self {
            secret: secret.as_ref().to_vec(),
        }
    }
}

impl fmt::Debug for MemorySigner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MemorySigner")
            .field("secret", &"<redacted>")
            .finish()
    }
}

impl Signer for MemorySigner {
    fn sign_hex(&self, payload: &[u8]) -> Result<String> {
        let mut mac = HmacSha256::new_from_slice(&self.secret)
            .map_err(|_| MarketError::new(ErrorKind::ConfigError, "invalid hmac key length"))?;
        mac.update(payload);
        Ok(hex::encode(mac.finalize().into_bytes()))
    }
}

/// Env-backed signer that reads the secret lazily on every signing call.
pub struct EnvSigner {
    secret_var: Box<str>,
}

impl EnvSigner {
    #[must_use]
    pub fn new(secret_var: impl Into<Box<str>>) -> Self {
        Self {
            secret_var: secret_var.into(),
        }
    }
}

impl fmt::Debug for EnvSigner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EnvSigner")
            .field("secret_var", &self.secret_var)
            .field("secret", &"<redacted>")
            .finish()
    }
}

impl Signer for EnvSigner {
    fn sign_hex(&self, payload: &[u8]) -> Result<String> {
        let secret = env::var(self.secret_var.as_ref()).map_err(|_| {
            MarketError::new(
                ErrorKind::AuthError,
                format!("missing env secret {}", self.secret_var),
            )
        })?;
        MemorySigner::new(secret).sign_hex(payload)
    }
}

#[cfg(test)]
mod tests {
    use super::{MemorySigner, Signer};

    #[test]
    fn memory_signer_produces_stable_hmac() {
        let signer = MemorySigner::new("secret");
        let signature = signer.sign_hex(b"payload");
        assert_eq!(
            signature.as_deref(),
            Ok("b82fcb791acec57859b989b430a826488ce2e479fdf92326bd0a2e8375a42ba4")
        );
    }
}
