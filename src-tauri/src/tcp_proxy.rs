use crate::model::WebService;
use crate::state::AppState;
use tokio::net::{TcpListener, TcpStream};

pub async fn bind(service: &WebService) -> anyhow::Result<TcpListener> {
    Ok(TcpListener::bind((service.local_address.as_str(), service.local_port)).await?)
}

pub async fn run(state: AppState, service: WebService, listener: TcpListener) {
    loop {
        let (socket, peer) = match listener.accept().await {
            Ok(connection) => connection,
            Err(error) => {
                state
                    .service_channel_failed(&service, format!("TCP 监听失败：{error}"))
                    .await;
                return;
            }
        };
        let task_state = state.clone();
        let task_service = service.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = forward(
                task_state.clone(),
                task_service.clone(),
                socket,
                peer.port(),
            )
            .await
            {
                task_state
                    .service_channel_failed(&task_service, error.to_string())
                    .await;
            }
        });
    }
}

async fn forward(
    state: AppState,
    service: WebService,
    mut socket: TcpStream,
    origin_port: u16,
) -> anyhow::Result<()> {
    let channel = state.open_direct_channel(&service, origin_port).await?;
    let mut remote = channel.into_stream();
    tokio::io::copy_bidirectional(&mut socket, &mut remote).await?;
    Ok(())
}
