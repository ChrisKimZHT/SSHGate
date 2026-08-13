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
            .unwrap_or(true))
    }
}

pub type SshHandle = client::Handle<ClientHandler>;
