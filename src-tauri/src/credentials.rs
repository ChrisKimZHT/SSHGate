use anyhow::{Context, Result};
use keyring::{Entry, Error};

const SERVICE: &str = "SSHGate";

fn account(server_id: &str) -> String {
    format!("server:{server_id}:ssh-secret")
}

pub async fn set_secret(server_id: &str, secret: &str) -> Result<()> {
    let account = account(server_id);
    let secret = secret.to_owned();
    tokio::task::spawn_blocking(move || {
        Entry::new(SERVICE, &account)?.set_password(&secret)
    })
    .await
    .context("系统凭据库任务执行失败")?
    .context("无法写入系统凭据库")
}

pub async fn get_secret(server_id: &str) -> Result<Option<String>> {
    let account = account(server_id);
    tokio::task::spawn_blocking(move || Entry::new(SERVICE, &account)?.get_password())
        .await
        .context("系统凭据库任务执行失败")?
        .map(Some)
        .or_else(|error| match error {
            Error::NoEntry => Ok(None),
            other => Err(other),
        })
        .context("无法读取系统凭据库")
}

pub async fn delete_secret(server_id: &str) -> Result<()> {
    let account = account(server_id);
    tokio::task::spawn_blocking(move || Entry::new(SERVICE, &account)?.delete_credential())
        .await
        .context("系统凭据库任务执行失败")?
        .or_else(|error| match error {
            Error::NoEntry => Ok(()),
            other => Err(other),
        })
        .context("无法删除系统凭据库中的凭据")
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::{delete_secret, get_secret, set_secret};

    #[tokio::test]
    async fn windows_credential_manager_round_trip() {
        let server_id = format!("test-{}", uuid::Uuid::new_v4());
        let secret = format!("sshgate-test-{}", uuid::Uuid::new_v4());

        set_secret(&server_id, &secret).await.unwrap();
        assert_eq!(get_secret(&server_id).await.unwrap().as_deref(), Some(secret.as_str()));
        delete_secret(&server_id).await.unwrap();
        assert!(get_secret(&server_id).await.unwrap().is_none());
    }
}
