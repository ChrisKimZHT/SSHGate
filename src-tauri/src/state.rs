use crate::credentials;
use crate::model::{
    AppConfig, AuthType, ConnectionStatus, RuntimeSnapshot, RuntimeState, ServiceStatus, Settings,
    SshServer, TerminalEvent, WebService,
};
use crate::proxy;
use crate::ssh::{ClientHandler, SshHandle};
use anyhow::{anyhow, Context};
use base64::Engine;
use russh::client;
use russh::keys::{load_secret_key, PrivateKeyWithHashAlg};
use russh::{Channel, ChannelMsg, Disconnect};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, Mutex, RwLock};
use uuid::Uuid;

#[derive(Clone)]
pub struct AppState(Arc<Inner>);

struct Inner {
    app: AppHandle,
    config_path: PathBuf,
    config: RwLock<AppConfig>,
    sessions: Mutex<HashMap<String, Arc<Mutex<SshHandle>>>>,
    terminals: Mutex<HashMap<String, TerminalEntry>>,
    server_states: RwLock<HashMap<String, RuntimeState<ConnectionStatus>>>,
    service_states: RwLock<HashMap<String, RuntimeState<ServiceStatus>>>,
    proxy_error: RwLock<Option<String>>,
    proxy_task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    shutting_down: AtomicBool,
}

struct TerminalEntry {
    server_id: String,
    sender: mpsc::Sender<TerminalControl>,
}

enum TerminalControl {
    Input(Vec<u8>),
    Resize(u32, u32),
    Close,
}

impl AppState {
    pub fn load(app: AppHandle, config_path: PathBuf) -> anyhow::Result<Self> {
        let config = if config_path.exists() {
            let raw = std::fs::read_to_string(&config_path)
                .with_context(|| format!("无法读取配置文件 {}", config_path.display()))?;
            serde_json::from_str(&raw).context("SSHGate 配置文件格式不正确")?
        } else {
            AppConfig::default()
        };

        let server_states = config
            .servers
            .iter()
            .map(|server| {
                (
                    server.id.clone(),
                    RuntimeState::new(ConnectionStatus::Stopped),
                )
            })
            .collect();
        let service_states = config
            .services
            .iter()
            .map(|service| {
                (
                    service.id.clone(),
                    RuntimeState::new(ServiceStatus::Stopped),
                )
            })
            .collect();

        Ok(Self(Arc::new(Inner {
            app,
            config_path,
            config: RwLock::new(config),
            sessions: Mutex::new(HashMap::new()),
            terminals: Mutex::new(HashMap::new()),
            server_states: RwLock::new(server_states),
            service_states: RwLock::new(service_states),
            proxy_error: RwLock::new(None),
            proxy_task: Mutex::new(None),
            shutting_down: AtomicBool::new(false),
        })))
    }

    pub async fn snapshot(&self) -> RuntimeSnapshot {
        RuntimeSnapshot {
            config: self.0.config.read().await.clone(),
            server_states: self.0.server_states.read().await.clone(),
            service_states: self.0.service_states.read().await.clone(),
            proxy_error: self.0.proxy_error.read().await.clone(),
        }
    }

    pub async fn emit_state(&self) {
        let _ = self.0.app.emit("state-changed", self.snapshot().await);
    }

    async fn persist(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.0.config_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let serialized = serde_json::to_string_pretty(&*self.0.config.read().await)?;
        let temporary = self.0.config_path.with_extension("json.tmp");
        tokio::fs::write(&temporary, serialized).await?;
        if self.0.config_path.exists() {
            tokio::fs::remove_file(&self.0.config_path).await?;
        }
        tokio::fs::rename(temporary, &self.0.config_path).await?;
        Ok(())
    }

    pub async fn set_proxy_error(&self, error: Option<String>) {
        *self.0.proxy_error.write().await = error;
        self.emit_state().await;
    }

    pub async fn restart_proxy(&self) {
        if let Some(task) = self.0.proxy_task.lock().await.take() {
            task.abort();
        }
        let settings = self.0.config.read().await.settings.clone();
        let state = self.clone();
        let task = tauri::async_runtime::spawn(async move {
            if let Err(error) = proxy::run(
                state.clone(),
                settings.listen_address.clone(),
                settings.listen_port,
            )
            .await
            {
                state
                    .set_proxy_error(Some(format!(
                        "无法监听 {}:{}：{}",
                        settings.listen_address, settings.listen_port, error
                    )))
                    .await;
            }
        });
        *self.0.proxy_task.lock().await = Some(task);
    }

    pub async fn restore_services(&self) {
        let config = self.0.config.read().await.clone();
        if !config.settings.auto_start_services {
            return;
        }
        let server_ids: HashSet<String> = config
            .services
            .iter()
            .filter(|service| service.desired_running)
            .map(|service| service.server_id.clone())
            .collect();
        for server_id in server_ids {
            let Some(server) = config.servers.iter().find(|server| server.id == server_id) else {
                continue;
            };
            if server.auth_type == AuthType::Key || server.remember_secret {
                let state = self.clone();
                let id = server_id.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = state.connect_internal(&id, None, false).await;
                });
            } else {
                self.set_server_services_state(
                    &server_id,
                    ServiceStatus::Error,
                    Some("未保存 SSH 密码，请手动重新连接".into()),
                )
                .await;
            }
        }
    }

    pub async fn save_server(
        &self,
        mut server: SshServer,
        secret: Option<String>,
    ) -> anyhow::Result<()> {
        validate_server(&server)?;
        server.name = server.name.trim().to_owned();
        server.host = server.host.trim().to_owned();
        server.username = server.username.trim().to_owned();
        server.private_key_path = server.private_key_path.trim().to_owned();
        if server.id.trim().is_empty() {
            server.id = Uuid::new_v4().to_string();
        }

        let (name_exists, existing) = {
            let config = self.0.config.read().await;
            (
                config.servers.iter().any(|item| {
                    item.name.eq_ignore_ascii_case(&server.name) && item.id != server.id
                }),
                config
                    .servers
                    .iter()
                    .find(|item| item.id == server.id)
                    .cloned(),
            )
        };
        if name_exists {
            return Err(anyhow!("服务器名称已存在"));
        }

        let secret = secret.filter(|value| !value.is_empty());
        if server.remember_secret {
            if let Some(secret) = secret.as_deref() {
                credentials::set_secret(&server.id, secret).await?;
            } else {
                let can_keep_existing = existing
                    .as_ref()
                    .map(|item| item.remember_secret && item.auth_type == server.auth_type)
                    .unwrap_or(false);
                if !can_keep_existing || credentials::get_secret(&server.id).await?.is_none() {
                    return Err(anyhow!(
                        "请输入要保存到系统凭据库的{}",
                        if server.auth_type == AuthType::Password {
                            "SSH 密码"
                        } else {
                            "私钥口令"
                        }
                    ));
                }
            }
        } else if existing
            .as_ref()
            .map(|item| item.remember_secret)
            .unwrap_or(false)
        {
            credentials::delete_secret(&server.id).await?;
        }

        let mut config = self.0.config.write().await;
        match config.servers.iter_mut().find(|item| item.id == server.id) {
            Some(existing) => *existing = server.clone(),
            None => config.servers.push(server.clone()),
        }
        drop(config);
        self.0
            .server_states
            .write()
            .await
            .entry(server.id)
            .or_insert_with(|| RuntimeState::new(ConnectionStatus::Stopped));
        self.persist().await?;
        self.emit_state().await;
        Ok(())
    }

    pub async fn remove_server(&self, server_id: &str) -> anyhow::Result<()> {
        let remembered = self
            .0
            .config
            .read()
            .await
            .servers
            .iter()
            .find(|server| server.id == server_id)
            .map(|server| server.remember_secret)
            .unwrap_or(false);
        if remembered {
            credentials::delete_secret(server_id).await?;
        }
        self.close_session(server_id).await;
        let terminal_ids: Vec<String> = self
            .0
            .terminals
            .lock()
            .await
            .iter()
            .filter(|(_, terminal)| terminal.server_id == server_id)
            .map(|(id, _)| id.clone())
            .collect();
        for terminal_id in terminal_ids {
            self.close_terminal(&terminal_id).await;
        }

        let removed_service_ids: Vec<String> = {
            let mut config = self.0.config.write().await;
            config.servers.retain(|server| server.id != server_id);
            let ids = config
                .services
                .iter()
                .filter(|service| service.server_id == server_id)
                .map(|service| service.id.clone())
                .collect();
            config
                .services
                .retain(|service| service.server_id != server_id);
            ids
        };
        self.0.server_states.write().await.remove(server_id);
        let mut service_states = self.0.service_states.write().await;
        for id in removed_service_ids {
            service_states.remove(&id);
        }
        drop(service_states);
        self.persist().await?;
        self.emit_state().await;
        Ok(())
    }

    pub async fn save_service(&self, mut service: WebService) -> anyhow::Result<()> {
        service.name = service.name.trim().to_owned();
        service.remote_host = service.remote_host.trim().to_owned();
        if service.id.trim().is_empty() {
            service.id = Uuid::new_v4().to_string();
        }
        let mut config = self.0.config.write().await;
        let server_name = config
            .servers
            .iter()
            .find(|server| server.id == service.server_id)
            .map(|server| server.name.clone())
            .ok_or_else(|| anyhow!("所选 SSH 服务器不存在"))?;
        service.domain = if service.domain.trim().is_empty() {
            default_service_domain(&service.name, &server_name)
        } else {
            normalize_domain(&service.domain)
        };
        validate_service(&service)?;
        if config
            .services
            .iter()
            .any(|item| item.domain.eq_ignore_ascii_case(&service.domain) && item.id != service.id)
        {
            return Err(anyhow!("域名 {} 已被其他应用使用", service.domain));
        }
        match config
            .services
            .iter_mut()
            .find(|item| item.id == service.id)
        {
            Some(existing) => {
                service.desired_running = existing.desired_running;
                *existing = service.clone();
            }
            None => config.services.push(service.clone()),
        }
        drop(config);
        self.0
            .service_states
            .write()
            .await
            .entry(service.id)
            .or_insert_with(|| RuntimeState::new(ServiceStatus::Stopped));
        self.persist().await?;
        self.emit_state().await;
        Ok(())
    }

    pub async fn remove_service(&self, service_id: &str) -> anyhow::Result<()> {
        let server_id = {
            let mut config = self.0.config.write().await;
            let server_id = config
                .services
                .iter()
                .find(|service| service.id == service_id)
                .map(|service| service.server_id.clone());
            config.services.retain(|service| service.id != service_id);
            server_id
        };
        self.0.service_states.write().await.remove(service_id);
        self.persist().await?;
        if let Some(server_id) = server_id {
            self.close_session_if_unused(&server_id).await;
        }
        self.emit_state().await;
        Ok(())
    }

    pub async fn save_settings(&self, settings: Settings) -> anyhow::Result<()> {
        if settings.listen_address.trim().is_empty() || settings.reconnect_delay_seconds == 0 {
            return Err(anyhow!("监听地址不能为空，重连间隔必须大于 0"));
        }
        self.0.config.write().await.settings = settings;
        self.persist().await?;
        self.restart_proxy().await;
        self.emit_state().await;
        Ok(())
    }

    pub async fn start_service(
        &self,
        service_id: &str,
        password: Option<String>,
    ) -> anyhow::Result<()> {
        let service = {
            let mut config = self.0.config.write().await;
            let service = config
                .services
                .iter_mut()
                .find(|service| service.id == service_id)
                .ok_or_else(|| anyhow!("应用不存在"))?;
            service.desired_running = true;
            service.clone()
        };
        self.0.service_states.write().await.insert(
            service.id.clone(),
            RuntimeState::new(ServiceStatus::Starting),
        );
        self.persist().await?;
        self.emit_state().await;

        if let Err(error) = self
            .connect_internal(&service.server_id, password, false)
            .await
        {
            self.0.service_states.write().await.insert(
                service.id.clone(),
                RuntimeState::error(ServiceStatus::Error, error.to_string()),
            );
            self.emit_state().await;
            return Err(error);
        }
        self.0
            .service_states
            .write()
            .await
            .insert(service.id, RuntimeState::new(ServiceStatus::Running));
        self.emit_state().await;
        Ok(())
    }

    pub async fn start_server_services(
        &self,
        server_id: &str,
        password: Option<String>,
    ) -> anyhow::Result<()> {
        let service_ids = {
            let mut config = self.0.config.write().await;
            if !config.servers.iter().any(|server| server.id == server_id) {
                return Err(anyhow!("SSH 服务器不存在"));
            }
            let mut ids = Vec::new();
            for service in config
                .services
                .iter_mut()
                .filter(|service| service.server_id == server_id)
            {
                service.desired_running = true;
                ids.push(service.id.clone());
            }
            ids
        };
        if service_ids.is_empty() {
            return Err(anyhow!("该服务器还没有应用"));
        }
        {
            let mut states = self.0.service_states.write().await;
            for id in &service_ids {
                states.insert(id.clone(), RuntimeState::new(ServiceStatus::Starting));
            }
        }
        self.persist().await?;
        self.emit_state().await;

        if let Err(error) = self.connect_internal(server_id, password, false).await {
            let mut states = self.0.service_states.write().await;
            for id in &service_ids {
                states.insert(
                    id.clone(),
                    RuntimeState::error(ServiceStatus::Error, error.to_string()),
                );
            }
            drop(states);
            self.emit_state().await;
            return Err(error);
        }
        {
            let mut states = self.0.service_states.write().await;
            for id in service_ids {
                states.insert(id, RuntimeState::new(ServiceStatus::Running));
            }
        }
        self.emit_state().await;
        Ok(())
    }

    pub async fn stop_service(&self, service_id: &str) -> anyhow::Result<()> {
        let mut config = self.0.config.write().await;
        let service = config
            .services
            .iter_mut()
            .find(|service| service.id == service_id)
            .ok_or_else(|| anyhow!("应用不存在"))?;
        service.desired_running = false;
        let server_id = service.server_id.clone();
        drop(config);
        self.0.service_states.write().await.insert(
            service_id.to_owned(),
            RuntimeState::new(ServiceStatus::Stopped),
        );
        self.persist().await?;
        self.close_session_if_unused(&server_id).await;
        self.emit_state().await;
        Ok(())
    }

    pub async fn stop_server_services(&self, server_id: &str) -> anyhow::Result<()> {
        let service_ids = {
            let mut config = self.0.config.write().await;
            if !config.servers.iter().any(|server| server.id == server_id) {
                return Err(anyhow!("SSH 服务器不存在"));
            }
            let mut ids = Vec::new();
            for service in config
                .services
                .iter_mut()
                .filter(|service| service.server_id == server_id)
            {
                service.desired_running = false;
                ids.push(service.id.clone());
            }
            ids
        };
        if service_ids.is_empty() {
            return Err(anyhow!("该服务器还没有应用"));
        }
        {
            let mut states = self.0.service_states.write().await;
            for id in service_ids {
                states.insert(id, RuntimeState::new(ServiceStatus::Stopped));
            }
        }
        self.persist().await?;
        self.close_session_if_unused(server_id).await;
        self.emit_state().await;
        Ok(())
    }

    pub async fn connect_server(
        &self,
        server_id: &str,
        password: Option<String>,
    ) -> anyhow::Result<()> {
        self.connect_internal(server_id, password, false)
            .await
            .map(|_| ())
    }

    async fn connect_internal(
        &self,
        server_id: &str,
        password: Option<String>,
        reconnecting: bool,
    ) -> anyhow::Result<Arc<Mutex<SshHandle>>> {
        if let Some(existing) = self.0.sessions.lock().await.get(server_id).cloned() {
            if !existing.lock().await.is_closed() {
                return Ok(existing);
            }
            self.0.sessions.lock().await.remove(server_id);
        }

        let server = self
            .0
            .config
            .read()
            .await
            .servers
            .iter()
            .find(|server| server.id == server_id)
            .cloned()
            .ok_or_else(|| anyhow!("SSH 服务器不存在"))?;

        let provided_secret = password.filter(|value| !value.is_empty());
        let password = if provided_secret.is_some() {
            provided_secret
        } else if server.remember_secret {
            match credentials::get_secret(&server.id).await {
                Ok(secret) => secret,
                Err(error) => {
                    let message = format!("无法读取已保存的 SSH 凭据：{error:#}");
                    self.connection_failed(&server.id, reconnecting, &message)
                        .await;
                    return Err(anyhow!(message));
                }
            }
        } else {
            None
        };

        let status = if reconnecting {
            ConnectionStatus::Reconnecting
        } else {
            ConnectionStatus::Connecting
        };
        self.0
            .server_states
            .write()
            .await
            .insert(server.id.clone(), RuntimeState::new(status));
        self.set_server_services_state(
            &server.id,
            if reconnecting {
                ServiceStatus::Reconnecting
            } else {
                ServiceStatus::Starting
            },
            None,
        )
        .await;
        self.emit_state().await;

        let observed = Arc::new(StdMutex::new(None));
        let handler = ClientHandler::new(server.host_key_fingerprint.clone(), observed.clone());
        let ssh_config = client::Config {
            inactivity_timeout: Some(Duration::from_secs(600)),
            keepalive_interval: Some(Duration::from_secs(20)),
            keepalive_max: 3,
            nodelay: true,
            ..Default::default()
        };
        let connection = client::connect(
            Arc::new(ssh_config),
            (server.host.as_str(), server.port),
            handler,
        )
        .await;
        let mut handle = match connection {
            Ok(handle) => handle,
            Err(error) => {
                let message = friendly_ssh_error(&server, &error.to_string());
                self.connection_failed(&server.id, reconnecting, &message)
                    .await;
                return Err(anyhow!(message));
            }
        };

        let authentication: anyhow::Result<bool> = async {
            match server.auth_type {
                AuthType::Key => {
                    let key_path = expand_home(&server.private_key_path);
                    let key =
                        load_secret_key(&key_path, password.as_deref()).with_context(|| {
                            format!(
                                "无法读取私钥 {}。请检查私钥路径或输入正确的私钥口令",
                                key_path.display()
                            )
                        })?;
                    let hash = handle.best_supported_rsa_hash().await?.flatten();
                    Ok(handle
                        .authenticate_publickey(
                            server.username.clone(),
                            PrivateKeyWithHashAlg::new(Arc::new(key), hash),
                        )
                        .await?
                        .success())
                }
                AuthType::Password => {
                    let password = password
                        .filter(|value| !value.is_empty())
                        .ok_or_else(|| anyhow!("请输入 SSH 密码"))?;
                    Ok(handle
                        .authenticate_password(server.username.clone(), password)
                        .await?
                        .success())
                }
            }
        }
        .await;
        let authenticated = match authentication {
            Ok(authenticated) => authenticated,
            Err(error) => {
                let message = format!("SSH 身份验证失败：{error:#}");
                self.connection_failed(&server.id, reconnecting, &message)
                    .await;
                return Err(anyhow!(message));
            }
        };

        if !authenticated {
            let message = "SSH 身份验证失败，请检查用户名和认证信息";
            self.connection_failed(&server.id, reconnecting, message)
                .await;
            return Err(anyhow!(message));
        }

        if server.host_key_fingerprint.is_none() {
            let fingerprint = observed.lock().ok().and_then(|value| value.clone());
            if let Some(fingerprint) = fingerprint {
                let mut config = self.0.config.write().await;
                if let Some(saved) = config.servers.iter_mut().find(|item| item.id == server.id) {
                    saved.host_key_fingerprint = Some(fingerprint);
                }
                drop(config);
                let _ = self.persist().await;
            }
        }

        let shared = Arc::new(Mutex::new(handle));
        self.0
            .sessions
            .lock()
            .await
            .insert(server.id.clone(), shared.clone());
        self.0.server_states.write().await.insert(
            server.id.clone(),
            RuntimeState::new(ConnectionStatus::Connected),
        );
        self.set_server_services_state(&server.id, ServiceStatus::Running, None)
            .await;
        self.emit_state().await;
        self.spawn_monitor(server.id, shared.clone());
        Ok(shared)
    }

    async fn connection_failed(&self, server_id: &str, reconnecting: bool, message: &str) {
        self.0.server_states.write().await.insert(
            server_id.to_owned(),
            RuntimeState::error(
                if reconnecting {
                    ConnectionStatus::Reconnecting
                } else {
                    ConnectionStatus::Error
                },
                message,
            ),
        );
        self.set_server_services_state(
            server_id,
            if reconnecting {
                ServiceStatus::Reconnecting
            } else {
                ServiceStatus::Error
            },
            Some(message.to_owned()),
        )
        .await;
        self.emit_state().await;
    }

    fn spawn_monitor(&self, server_id: String, watched: Arc<Mutex<SshHandle>>) {
        let state = self.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(2)).await;
                if state.0.shutting_down.load(Ordering::SeqCst) {
                    return;
                }
                if !watched.lock().await.is_closed() {
                    continue;
                }

                let mut sessions = state.0.sessions.lock().await;
                let is_current = sessions
                    .get(&server_id)
                    .map(|current| Arc::ptr_eq(current, &watched))
                    .unwrap_or(false);
                if is_current {
                    sessions.remove(&server_id);
                }
                drop(sessions);
                if !is_current {
                    return;
                }

                let config = state.0.config.read().await.clone();
                let should_reconnect = config
                    .services
                    .iter()
                    .any(|service| service.server_id == server_id && service.desired_running);
                if !should_reconnect {
                    state.0.server_states.write().await.insert(
                        server_id.clone(),
                        RuntimeState::new(ConnectionStatus::Stopped),
                    );
                    state.emit_state().await;
                    return;
                }
                let Some(server) = config.servers.iter().find(|server| server.id == server_id)
                else {
                    return;
                };
                if server.auth_type == AuthType::Password && !server.remember_secret {
                    let message = "SSH 已断开；密码未保存，请手动重新连接";
                    state.connection_failed(&server_id, false, message).await;
                    return;
                }

                loop {
                    if state.0.shutting_down.load(Ordering::SeqCst) {
                        return;
                    }
                    let still_desired =
                        state.0.config.read().await.services.iter().any(|service| {
                            service.server_id == server_id && service.desired_running
                        });
                    if !still_desired {
                        return;
                    }
                    let delay = state.0.config.read().await.settings.reconnect_delay_seconds;
                    tokio::time::sleep(Duration::from_secs(delay)).await;
                    if state.connect_internal(&server_id, None, true).await.is_ok() {
                        return;
                    }
                }
            }
        });
    }

    pub async fn disconnect_server(&self, server_id: &str) -> anyhow::Result<()> {
        {
            let mut config = self.0.config.write().await;
            for service in config
                .services
                .iter_mut()
                .filter(|service| service.server_id == server_id)
            {
                service.desired_running = false;
            }
        }
        self.close_session(server_id).await;
        self.0.server_states.write().await.insert(
            server_id.to_owned(),
            RuntimeState::new(ConnectionStatus::Stopped),
        );
        self.set_server_services_state(server_id, ServiceStatus::Stopped, None)
            .await;
        self.persist().await?;
        self.emit_state().await;
        Ok(())
    }

    async fn close_session(&self, server_id: &str) {
        if let Some(handle) = self.0.sessions.lock().await.remove(server_id) {
            let _ = handle
                .lock()
                .await
                .disconnect(Disconnect::ByApplication, "SSHGate disconnect", "en")
                .await;
        }
    }

    async fn close_session_if_unused(&self, server_id: &str) {
        let has_running_app = self
            .0
            .config
            .read()
            .await
            .services
            .iter()
            .any(|service| service.server_id == server_id && service.desired_running);
        if has_running_app {
            return;
        }
        let has_terminal = self
            .0
            .terminals
            .lock()
            .await
            .values()
            .any(|terminal| terminal.server_id == server_id);
        if has_terminal {
            return;
        }
        self.close_session(server_id).await;
        self.0.server_states.write().await.insert(
            server_id.to_owned(),
            RuntimeState::new(ConnectionStatus::Stopped),
        );
    }

    async fn set_server_services_state(
        &self,
        server_id: &str,
        status: ServiceStatus,
        error: Option<String>,
    ) {
        let service_ids: Vec<String> = self
            .0
            .config
            .read()
            .await
            .services
            .iter()
            .filter(|service| service.server_id == server_id && service.desired_running)
            .map(|service| service.id.clone())
            .collect();
        let mut states = self.0.service_states.write().await;
        for id in service_ids {
            states.insert(
                id,
                RuntimeState {
                    status,
                    error: error.clone(),
                },
            );
        }
    }

    pub async fn find_running_service(&self, host: &str) -> Option<WebService> {
        let config = self.0.config.read().await;
        config
            .services
            .iter()
            .find(|service| service.desired_running && service.domain.eq_ignore_ascii_case(host))
            .cloned()
    }

    pub async fn open_direct_channel(
        &self,
        service: &WebService,
        origin_port: u16,
    ) -> anyhow::Result<Channel<client::Msg>> {
        let handle = self.ensure_session(&service.server_id).await?;
        let channel = handle
            .lock()
            .await
            .channel_open_direct_tcpip(
                &service.remote_host,
                service.remote_port as u32,
                "127.0.0.1",
                origin_port as u32,
            )
            .await
            .context("SSH direct-tcpip channel 打开失败")?;
        let should_emit = self
            .0
            .service_states
            .read()
            .await
            .get(&service.id)
            .map(|state| state.status != ServiceStatus::Running)
            .unwrap_or(true);
        if should_emit {
            self.0.service_states.write().await.insert(
                service.id.clone(),
                RuntimeState::new(ServiceStatus::Running),
            );
            self.emit_state().await;
        }
        Ok(channel)
    }

    async fn ensure_session(&self, server_id: &str) -> anyhow::Result<Arc<Mutex<SshHandle>>> {
        if let Some(handle) = self.0.sessions.lock().await.get(server_id).cloned() {
            if !handle.lock().await.is_closed() {
                return Ok(handle);
            }
        }
        let auth_type = self
            .0
            .config
            .read()
            .await
            .servers
            .iter()
            .find(|server| server.id == server_id)
            .map(|server| server.auth_type.clone())
            .ok_or_else(|| anyhow!("SSH 服务器不存在"))?;
        if auth_type == AuthType::Password {
            return Err(anyhow!("SSH 会话已断开，请回到服务器页重新输入密码"));
        }
        self.connect_internal(server_id, None, true).await
    }

    pub async fn service_channel_failed(&self, service: &WebService, message: String) {
        self.0.service_states.write().await.insert(
            service.id.clone(),
            RuntimeState::error(ServiceStatus::Error, message),
        );
        self.emit_state().await;
    }

    pub async fn open_terminal(
        &self,
        server_id: &str,
        terminal_id: &str,
        cols: u32,
        rows: u32,
        password: Option<String>,
    ) -> anyhow::Result<()> {
        if self.0.terminals.lock().await.contains_key(terminal_id) {
            return Ok(());
        }
        let handle = match self.0.sessions.lock().await.get(server_id).cloned() {
            Some(handle) if !handle.lock().await.is_closed() => handle,
            _ => self.connect_internal(server_id, password, false).await?,
        };
        let channel_result: anyhow::Result<Channel<client::Msg>> = async {
            let channel = handle
                .lock()
                .await
                .channel_open_session()
                .await
                .context("无法打开 SSH session channel")?;
            channel
                .request_pty(false, "xterm-256color", cols, rows, 0, 0, &[])
                .await
                .context("远端拒绝 PTY 请求")?;
            channel
                .request_shell(false)
                .await
                .context("远端拒绝 Shell 请求")?;
            Ok(channel)
        }
        .await;
        let channel = match channel_result {
            Ok(channel) => channel,
            Err(error) => {
                self.close_session_if_unused(server_id).await;
                self.emit_state().await;
                return Err(error);
            }
        };

        let (sender, mut receiver) = mpsc::channel::<TerminalControl>(128);
        self.0.terminals.lock().await.insert(
            terminal_id.to_owned(),
            TerminalEntry {
                server_id: server_id.to_owned(),
                sender,
            },
        );
        let state = self.clone();
        let id = terminal_id.to_owned();
        let terminal_server_id = server_id.to_owned();
        tauri::async_runtime::spawn(async move {
            let mut channel = channel;
            let mut exit_status = None;
            let mut close_message = None;
            loop {
                tokio::select! {
                    message = channel.wait() => match message {
                        Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, .. }) => {
                            let encoded = base64::engine::general_purpose::STANDARD.encode(&data[..]);
                            let _ = state.0.app.emit("terminal-output", TerminalEvent { terminal_id: id.clone(), data: Some(encoded), message: None, exit_status: None });
                        }
                        Some(ChannelMsg::ExitStatus { exit_status: status }) => exit_status = Some(status),
                        Some(ChannelMsg::ExitSignal { error_message, .. }) => {
                            close_message = Some(if error_message.is_empty() { "远端 Shell 被信号终止".into() } else { error_message });
                            break;
                        }
                        Some(ChannelMsg::Failure) => {
                            close_message = Some("远端拒绝了 PTY 或 Shell 请求".into());
                            break;
                        }
                        Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) => {
                            close_message.get_or_insert_with(|| "远端 Shell 已关闭".into());
                            break;
                        }
                        None => {
                            close_message.get_or_insert_with(|| "SSH channel 已关闭".into());
                            break;
                        }
                        _ => {}
                    },
                    control = receiver.recv() => match control {
                        Some(TerminalControl::Input(data)) => {
                            if channel.data_bytes(data).await.is_err() { break; }
                        }
                        Some(TerminalControl::Resize(cols, rows)) => {
                            let _ = channel.window_change(cols, rows, 0, 0).await;
                        }
                        Some(TerminalControl::Close) | None => {
                            let _ = channel.eof().await;
                            let _ = channel.close().await;
                            break;
                        }
                    }
                }
            }
            state.0.terminals.lock().await.remove(&id);
            state.close_session_if_unused(&terminal_server_id).await;
            state.emit_state().await;
            let _ = state.0.app.emit(
                "terminal-closed",
                TerminalEvent {
                    terminal_id: id,
                    data: None,
                    message: close_message,
                    exit_status,
                },
            );
        });
        Ok(())
    }

    pub async fn terminal_input(&self, terminal_id: &str, data: String) -> anyhow::Result<()> {
        let sender = self
            .0
            .terminals
            .lock()
            .await
            .get(terminal_id)
            .map(|entry| entry.sender.clone())
            .ok_or_else(|| anyhow!("终端已关闭"))?;
        sender
            .send(TerminalControl::Input(data.into_bytes()))
            .await
            .map_err(|_| anyhow!("终端已关闭"))
    }

    pub async fn terminal_resize(
        &self,
        terminal_id: &str,
        cols: u32,
        rows: u32,
    ) -> anyhow::Result<()> {
        let sender = self
            .0
            .terminals
            .lock()
            .await
            .get(terminal_id)
            .map(|entry| entry.sender.clone())
            .ok_or_else(|| anyhow!("终端已关闭"))?;
        sender
            .send(TerminalControl::Resize(cols.max(1), rows.max(1)))
            .await
            .map_err(|_| anyhow!("终端已关闭"))
    }

    pub async fn close_terminal(&self, terminal_id: &str) {
        let entry = self.0.terminals.lock().await.remove(terminal_id);
        if let Some(entry) = entry {
            let server_id = entry.server_id.clone();
            let _ = entry.sender.send(TerminalControl::Close).await;
            self.close_session_if_unused(&server_id).await;
            self.emit_state().await;
        }
    }

    pub async fn import_ssh_config(&self) -> anyhow::Result<Vec<SshServer>> {
        let path = dirs::home_dir()
            .ok_or_else(|| anyhow!("无法确定用户主目录"))?
            .join(".ssh")
            .join("config");
        let raw = tokio::fs::read_to_string(&path)
            .await
            .with_context(|| format!("无法读取 {}", path.display()))?;
        let imported = parse_ssh_config(&raw);
        let mut added = Vec::new();
        {
            let mut config = self.0.config.write().await;
            for server in imported {
                if config
                    .servers
                    .iter()
                    .any(|existing| existing.name.eq_ignore_ascii_case(&server.name))
                {
                    continue;
                }
                config.servers.push(server.clone());
                added.push(server);
            }
        }
        let mut states = self.0.server_states.write().await;
        for server in &added {
            states.insert(
                server.id.clone(),
                RuntimeState::new(ConnectionStatus::Stopped),
            );
        }
        drop(states);
        self.persist().await?;
        self.emit_state().await;
        Ok(added)
    }

    pub async fn shutdown(&self) {
        self.0.shutting_down.store(true, Ordering::SeqCst);
        if let Some(task) = self.0.proxy_task.lock().await.take() {
            task.abort();
        }
        let terminal_ids: Vec<String> = self.0.terminals.lock().await.keys().cloned().collect();
        for id in terminal_ids {
            self.close_terminal(&id).await;
        }
        let sessions: Vec<Arc<Mutex<SshHandle>>> = self
            .0
            .sessions
            .lock()
            .await
            .drain()
            .map(|(_, handle)| handle)
            .collect();
        for session in sessions {
            let _ = session
                .lock()
                .await
                .disconnect(Disconnect::ByApplication, "SSHGate exiting", "en")
                .await;
        }
    }
}

fn validate_server(server: &SshServer) -> anyhow::Result<()> {
    if server.name.trim().is_empty()
        || server.host.trim().is_empty()
        || server.username.trim().is_empty()
    {
        return Err(anyhow!("名称、主机和用户名不能为空"));
    }
    if server.auth_type == AuthType::Key && server.private_key_path.trim().is_empty() {
        return Err(anyhow!("私钥认证需要填写私钥路径"));
    }
    Ok(())
}

fn normalize_domain(domain: &str) -> String {
    let lower = domain.trim().to_ascii_lowercase();
    let without_scheme = lower
        .strip_prefix("http://")
        .or_else(|| lower.strip_prefix("https://"))
        .unwrap_or(&lower);
    let without_trailing = without_scheme.trim_end_matches(|ch| ch == '/' || ch == '.');
    let prefix = without_trailing
        .strip_suffix(".localhost")
        .unwrap_or(without_trailing);
    let normalized = prefix
        .split('.')
        .map(|label| normalize_domain_label(label, ""))
        .filter(|label| !label.is_empty())
        .collect::<Vec<_>>()
        .join(".");
    format!("{normalized}.localhost")
}

fn normalize_domain_label(value: &str, fallback: &str) -> String {
    let mut result = String::new();
    let mut last_was_separator = false;
    for ch in value.trim().to_ascii_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            result.push(ch);
            last_was_separator = false;
        } else if !last_was_separator && !result.is_empty() {
            result.push('-');
            last_was_separator = true;
        }
    }
    while result.ends_with('-') {
        result.pop();
    }
    if result.is_empty() {
        fallback.to_owned()
    } else {
        result
    }
}

fn default_service_domain(service_name: &str, server_name: &str) -> String {
    format!(
        "{}.{}.localhost",
        normalize_domain_label(service_name, "service"),
        normalize_domain_label(server_name, "server")
    )
}

fn validate_service(service: &WebService) -> anyhow::Result<()> {
    if service.name.is_empty() || service.remote_host.is_empty() || service.domain.is_empty() {
        return Err(anyhow!("名称、远端主机和域名不能为空"));
    }
    if !service.domain.ends_with(".localhost")
        || service.domain.contains(':')
        || service.domain.contains('/')
        || service.domain.contains(' ')
    {
        return Err(anyhow!(
            "域名必须是有效的 .localhost 域名，例如 jupyter.gpu.localhost"
        ));
    }
    if service.domain.split('.').any(|label| {
        label.is_empty()
            || label.starts_with('-')
            || label.ends_with('-')
            || !label
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
    }) {
        return Err(anyhow!("域名包含无效字符"));
    }
    Ok(())
}

fn expand_home(value: &str) -> PathBuf {
    if value == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(value));
    }
    if let Some(relative) = value
        .strip_prefix("~/")
        .or_else(|| value.strip_prefix("~\\"))
    {
        if let Some(home) = dirs::home_dir() {
            return home.join(relative);
        }
    }
    PathBuf::from(value)
}

fn friendly_ssh_error(server: &SshServer, error: &str) -> String {
    format!("无法连接 {}:{}：{}", server.host, server.port, error)
}

fn parse_ssh_config(raw: &str) -> Vec<SshServer> {
    #[derive(Default)]
    struct Entry {
        alias: String,
        hostname: Option<String>,
        port: Option<u16>,
        user: Option<String>,
        identity: Option<String>,
    }
    fn finish(entry: Entry, output: &mut Vec<SshServer>) {
        if entry.alias.is_empty()
            || entry.alias.contains('*')
            || entry.alias.contains('?')
            || entry.alias.contains('!')
            || entry.alias.split_whitespace().count() != 1
        {
            return;
        }
        output.push(SshServer {
            id: Uuid::new_v4().to_string(),
            name: entry.alias.clone(),
            host: entry.hostname.unwrap_or(entry.alias),
            port: entry.port.unwrap_or(22),
            username: entry.user.unwrap_or_default(),
            auth_type: AuthType::Key,
            private_key_path: entry.identity.unwrap_or_else(|| "~/.ssh/id_ed25519".into()),
            remember_secret: false,
            host_key_fingerprint: None,
        });
    }
    let mut output = Vec::new();
    let mut current = Entry::default();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(2, |ch: char| ch.is_whitespace() || ch == '=');
        let key = parts.next().unwrap_or_default().trim().to_ascii_lowercase();
        let value = parts
            .next()
            .unwrap_or_default()
            .trim()
            .trim_matches('"')
            .to_owned();
        match key.as_str() {
            "host" => {
                finish(current, &mut output);
                current = Entry {
                    alias: value,
                    ..Default::default()
                };
            }
            "hostname" => current.hostname = Some(value),
            "port" => current.port = value.parse().ok(),
            "user" => current.user = Some(value),
            "identityfile" => current.identity = Some(value),
            _ => {}
        }
    }
    finish(current, &mut output);
    output.retain(|server| !server.username.is_empty());
    output
}

#[cfg(test)]
mod tests {
    use super::{default_service_domain, normalize_domain, parse_ssh_config};

    #[test]
    fn imports_concrete_ssh_hosts_only() {
        let config = "Host *\n  ServerAliveInterval 30\nHost gpu\n HostName 10.0.0.8\n User root\n Port 2222\n IdentityFile ~/.ssh/gpu\n";
        let result = parse_ssh_config(config);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "gpu");
        assert_eq!(result[0].port, 2222);
    }

    #[test]
    fn normalizes_domain() {
        assert_eq!(
            normalize_domain(" Jupyter.GPU.localhost. "),
            "jupyter.gpu.localhost"
        );
        assert_eq!(
            normalize_domain("HTTP://My App.GPU_Server.localhost/"),
            "my-app.gpu-server.localhost"
        );
    }

    #[test]
    fn creates_default_service_domain() {
        assert_eq!(
            default_service_domain("My Jupyter!", "GPU_Server A"),
            "my-jupyter.gpu-server-a.localhost"
        );
        assert_eq!(
            default_service_domain("中文应用", "服务器"),
            "service.server.localhost"
        );
    }
}
