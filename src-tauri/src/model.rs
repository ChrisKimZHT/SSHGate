use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshServer {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_type: AuthType,
    #[serde(default)]
    pub private_key_path: String,
    #[serde(default)]
    pub host_key_fingerprint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AuthType {
    Key,
    Password,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebService {
    pub id: String,
    pub server_id: String,
    pub name: String,
    pub remote_host: String,
    pub remote_port: u16,
    pub domain: String,
    #[serde(default)]
    pub desired_running: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub listen_address: String,
    pub listen_port: u16,
    pub reconnect_delay_seconds: u64,
    pub auto_start_services: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            listen_address: "127.0.0.1".into(),
            listen_port: 80,
            reconnect_delay_seconds: 3,
            auto_start_services: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    #[serde(default)]
    pub servers: Vec<SshServer>,
    #[serde(default)]
    pub services: Vec<WebService>,
    #[serde(default)]
    pub settings: Settings,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ConnectionStatus {
    Stopped,
    Connecting,
    Connected,
    Error,
    Reconnecting,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ServiceStatus {
    Stopped,
    Starting,
    Running,
    Error,
    Reconnecting,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeState<T> {
    pub status: T,
    pub error: Option<String>,
}

impl<T> RuntimeState<T> {
    pub fn new(status: T) -> Self {
        Self {
            status,
            error: None,
        }
    }

    pub fn error(status: T, error: impl Into<String>) -> Self {
        Self {
            status,
            error: Some(error.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSnapshot {
    pub config: AppConfig,
    pub server_states: HashMap<String, RuntimeState<ConnectionStatus>>,
    pub service_states: HashMap<String, RuntimeState<ServiceStatus>>,
    pub proxy_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalEvent {
    pub terminal_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_status: Option<u32>,
}
