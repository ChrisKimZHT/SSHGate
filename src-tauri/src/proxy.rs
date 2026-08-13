use crate::state::AppState;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const MAX_HEADER_BYTES: usize = 64 * 1024;

pub async fn run(state: AppState, address: String, port: u16) -> anyhow::Result<()> {
    let listener = TcpListener::bind((address.as_str(), port)).await?;
    state.set_proxy_error(None).await;

    loop {
        let (socket, peer) = listener.accept().await?;
        let task_state = state.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = handle_connection(task_state, socket, peer.port()).await {
                eprintln!("proxy connection failed: {error:#}");
            }
        });
    }
}

async fn handle_connection(
    state: AppState,
    mut socket: TcpStream,
    origin_port: u16,
) -> anyhow::Result<()> {
    let mut initial = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];

    loop {
        let count =
            tokio::time::timeout(Duration::from_secs(10), socket.read(&mut chunk)).await??;
        if count == 0 {
            return Ok(());
        }
        initial.extend_from_slice(&chunk[..count]);
        if initial.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
        if initial.len() >= MAX_HEADER_BYTES {
            write_error(
                &mut socket,
                431,
                "Request Header Fields Too Large",
                "HTTP headers exceed 64 KiB",
            )
            .await?;
            return Ok(());
        }
    }

    let host =
        parse_host(&initial).ok_or_else(|| anyhow::anyhow!("request is missing a Host header"))?;
    let Some(service) = state.find_running_service(&host).await else {
        write_error(
            &mut socket,
            404,
            "Not Found",
            &format!("No running SSHGate application matches {host}"),
        )
        .await?;
        return Ok(());
    };

    match state.open_direct_channel(&service, origin_port).await {
        Ok(channel) => {
            let mut remote = channel.into_stream();
            remote.write_all(&initial).await?;
            remote.flush().await?;
            tokio::io::copy_bidirectional(&mut socket, &mut remote).await?;
        }
        Err(error) => {
            state
                .service_channel_failed(&service, error.to_string())
                .await;
            write_error(&mut socket, 502, "Bad Gateway", &error.to_string()).await?;
        }
    }
    Ok(())
}

fn parse_host(bytes: &[u8]) -> Option<String> {
    let header_end = bytes.windows(4).position(|window| window == b"\r\n\r\n")? + 4;
    let header = std::str::from_utf8(&bytes[..header_end]).ok()?;
    header.lines().skip(1).find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if !name.eq_ignore_ascii_case("host") {
            return None;
        }
        let value = value.trim().to_ascii_lowercase();
        Some(
            value
                .rsplit_once(':')
                .map(|(host, _)| host.to_owned())
                .unwrap_or(value),
        )
    })
}

async fn write_error(
    socket: &mut TcpStream,
    code: u16,
    reason: &str,
    detail: &str,
) -> std::io::Result<()> {
    let escaped = detail
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let body = format!("<!doctype html><meta charset=utf-8><title>SSHGate {code}</title><style>body{{font:16px system-ui;max-width:680px;margin:12vh auto;padding:24px;background:#0d1217;color:#dce5e9}}code{{color:#70e1b2}}</style><h1>SSHGate · {reason}</h1><p><code>{escaped}</code></p>");
    let response = format!("HTTP/1.1 {code} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{body}", body.len());
    socket.write_all(response.as_bytes()).await
}

#[cfg(test)]
mod tests {
    use super::parse_host;

    #[test]
    fn extracts_nested_localhost_domain() {
        let request = b"GET / HTTP/1.1\r\nHost: jupyter.gpu.localhost\r\nConnection: close\r\n\r\n";
        assert_eq!(
            parse_host(request).as_deref(),
            Some("jupyter.gpu.localhost")
        );
    }

    #[test]
    fn removes_explicit_local_port() {
        let request = b"GET / HTTP/1.1\r\nhost: grafana.server-a.localhost:8080\r\n\r\n";
        assert_eq!(
            parse_host(request).as_deref(),
            Some("grafana.server-a.localhost")
        );
    }
}
