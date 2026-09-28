use russh::client;
use russh::keys::PublicKey;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct ClientHandler {
    expected_fingerprint: Option<String>,
    observed_fingerprint: Arc<Mutex<Option<String>>>,
}

impl ClientHandler {
    pub fn new(
        expected_fingerprint: Option<String>,
        observed_fingerprint: Arc<Mutex<Option<String>>>,
    ) -> Self {
        Self {
            expected_fingerprint,
            observed_fingerprint,
        }
    }
}

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKey,
    ) -> Result<bool, Self::Error> {
        let fingerprint = server_public_key
            .fingerprint(Default::default())
            .to_string();
        if let Ok(mut observed) = self.observed_fingerprint.lock() {
            *observed = Some(fingerprint.clone());
        }
        Ok(self
            .expected_fingerprint
            .as_ref()
            .map(|expected| expected == &fingerprint)
            .unwrap_or(false))
    }
}

pub type SshHandle = client::Handle<ClientHandler>;

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use russh::client::Handler;

    fn public_key(byte: u8) -> PublicKey {
        let mut wire = Vec::new();
        wire.extend_from_slice(&11u32.to_be_bytes());
        wire.extend_from_slice(b"ssh-ed25519");
        wire.extend_from_slice(&32u32.to_be_bytes());
        wire.extend_from_slice(&[byte; 32]);
        PublicKey::from_openssh(&format!(
            "ssh-ed25519 {}",
            base64::engine::general_purpose::STANDARD.encode(wire)
        ))
        .unwrap()
    }

    #[tokio::test]
    async fn unknown_key_is_observed_but_rejected() {
        let key = public_key(1);
        let observed = Arc::new(Mutex::new(None));
        let mut handler = ClientHandler::new(None, observed.clone());
        assert!(!handler.check_server_key(&key).await.unwrap());
        assert_eq!(
            *observed.lock().unwrap(),
            Some(key.fingerprint(Default::default()).to_string())
        );
    }

    #[tokio::test]
    async fn confirmed_key_is_pinned_including_subsequent_key_checks() {
        let key = public_key(1);
        let changed = public_key(2);
        let observed = Arc::new(Mutex::new(None));
        let mut handler = ClientHandler::new(
            Some(key.fingerprint(Default::default()).to_string()),
            observed.clone(),
        );
        assert!(handler.check_server_key(&key).await.unwrap());
        assert!(!handler.check_server_key(&changed).await.unwrap());
        assert_eq!(
            *observed.lock().unwrap(),
            Some(changed.fingerprint(Default::default()).to_string())
        );
        assert!(handler.check_server_key(&key).await.unwrap());
    }
}
