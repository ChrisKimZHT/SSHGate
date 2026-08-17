mod credentials;
mod model;
mod proxy;
mod ssh;
mod state;
mod tcp_proxy;

use model::{RuntimeSnapshot, Settings, SshServer, WebService};
use state::AppState;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, State,
};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

type CommandResult<T> = Result<T, String>;

#[tauri::command]
async fn get_snapshot(state: State<'_, AppState>) -> CommandResult<RuntimeSnapshot> {
    Ok(state.snapshot().await)
}

#[tauri::command]
async fn save_server(
    state: State<'_, AppState>,
    server: SshServer,
    secret: Option<String>,
) -> CommandResult<RuntimeSnapshot> {
    state
        .save_server(server, secret)
        .await
        .map_err(format_error)?;
    Ok(state.snapshot().await)
}

#[tauri::command]
async fn remove_server(
    state: State<'_, AppState>,
    server_id: String,
) -> CommandResult<RuntimeSnapshot> {
    state
        .remove_server(&server_id)
        .await
        .map_err(format_error)?;
    Ok(state.snapshot().await)
}

#[tauri::command]
async fn save_sort_order(
    state: State<'_, AppState>,
    server_ids: Vec<String>,
    service_ids: Vec<String>,
) -> CommandResult<RuntimeSnapshot> {
    state
        .save_sort_order(server_ids, service_ids)
        .await
        .map_err(format_error)?;
    Ok(state.snapshot().await)
}

#[tauri::command]
async fn connect_server(
    state: State<'_, AppState>,
    server_id: String,
    password: Option<String>,
) -> CommandResult<RuntimeSnapshot> {
    state
        .connect_server(&server_id, password)
        .await
        .map_err(format_error)?;
    Ok(state.snapshot().await)
}

#[tauri::command]
async fn disconnect_server(
    state: State<'_, AppState>,
    server_id: String,
) -> CommandResult<RuntimeSnapshot> {
    state
        .disconnect_server(&server_id)
        .await
        .map_err(format_error)?;
    Ok(state.snapshot().await)
}

#[tauri::command]
async fn clear_server_fingerprint(
    state: State<'_, AppState>,
    server_id: String,
) -> CommandResult<RuntimeSnapshot> {
    state
        .clear_server_fingerprint(&server_id)
        .await
        .map_err(format_error)?;
    Ok(state.snapshot().await)
}

#[tauri::command]
async fn save_service(
    state: State<'_, AppState>,
    service: WebService,
) -> CommandResult<RuntimeSnapshot> {
    state.save_service(service).await.map_err(format_error)?;
    Ok(state.snapshot().await)
}

#[tauri::command]
async fn remove_service(
    state: State<'_, AppState>,
    service_id: String,
) -> CommandResult<RuntimeSnapshot> {
    state
        .remove_service(&service_id)
        .await
        .map_err(format_error)?;
    Ok(state.snapshot().await)
}

#[tauri::command]
async fn start_service(
    state: State<'_, AppState>,
    service_id: String,
    password: Option<String>,
) -> CommandResult<RuntimeSnapshot> {
    state
        .start_service(&service_id, password)
        .await
        .map_err(format_error)?;
    Ok(state.snapshot().await)
}

#[tauri::command]
async fn start_server_services(
    state: State<'_, AppState>,
    server_id: String,
    password: Option<String>,
) -> CommandResult<RuntimeSnapshot> {
    state
        .start_server_services(&server_id, password)
        .await
        .map_err(format_error)?;
    Ok(state.snapshot().await)
}

#[tauri::command]
async fn stop_server_services(
    state: State<'_, AppState>,
    server_id: String,
) -> CommandResult<RuntimeSnapshot> {
    state
        .stop_server_services(&server_id)
        .await
        .map_err(format_error)?;
    Ok(state.snapshot().await)
}

#[tauri::command]
async fn stop_service(
    state: State<'_, AppState>,
    service_id: String,
) -> CommandResult<RuntimeSnapshot> {
    state
        .stop_service(&service_id)
        .await
        .map_err(format_error)?;
    Ok(state.snapshot().await)
}

#[tauri::command]
async fn save_settings(
    state: State<'_, AppState>,
    settings: Settings,
) -> CommandResult<RuntimeSnapshot> {
    state.save_settings(settings).await.map_err(format_error)?;
    Ok(state.snapshot().await)
}

#[tauri::command]
async fn open_terminal(
    state: State<'_, AppState>,
    server_id: String,
    terminal_id: String,
    cols: u32,
    rows: u32,
    password: Option<String>,
) -> CommandResult<()> {
    state
        .open_terminal(&server_id, &terminal_id, cols, rows, password)
        .await
        .map_err(format_error)
}

#[tauri::command]
async fn terminal_input(
    state: State<'_, AppState>,
    terminal_id: String,
    data: String,
) -> CommandResult<()> {
    state
        .terminal_input(&terminal_id, data)
        .await
        .map_err(format_error)
}

#[tauri::command]
async fn terminal_resize(
    state: State<'_, AppState>,
    terminal_id: String,
    cols: u32,
    rows: u32,
) -> CommandResult<()> {
    state
        .terminal_resize(&terminal_id, cols, rows)
        .await
        .map_err(format_error)
}

#[tauri::command]
async fn close_terminal(state: State<'_, AppState>, terminal_id: String) -> CommandResult<()> {
    state.close_terminal(&terminal_id).await;
    Ok(())
}

#[tauri::command]
async fn import_ssh_config(state: State<'_, AppState>) -> CommandResult<Vec<SshServer>> {
    state.import_ssh_config().await.map_err(format_error)
}

#[tauri::command]
async fn import_app_config(
    state: State<'_, AppState>,
    path: String,
) -> CommandResult<RuntimeSnapshot> {
    state
        .import_app_config(&path)
        .await
        .map_err(format_error)?;
    Ok(state.snapshot().await)
}

#[tauri::command]
async fn export_app_config(state: State<'_, AppState>, path: String) -> CommandResult<()> {
    state.export_app_config(&path).await.map_err(format_error)
}

fn format_error(error: anyhow::Error) -> String {
    format!("{error:#}")
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "tray_show", "显示 SSHGate", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "tray_quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    let mut tray = TrayIconBuilder::new()
        .tooltip("SSHGate")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "tray_show" => show_main_window(app),
            "tray_quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } | TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                }
            ) {
                show_main_window(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }

    tray.build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let config_path = app.path().app_config_dir()?.join("config.json");
            let state = AppState::load(app.handle().clone(), config_path)?;
            app.manage(state.clone());
            setup_tray(app.handle())?;
            tauri::async_runtime::spawn(async move {
                state.restart_proxy().await;
                state.restore_services().await;
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }

            match event {
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    api.prevent_close();
                    let app = window.app_handle().clone();
                    app.dialog()
                        .message("确定要退出 SSHGate 吗？正在运行的连接和服务将会停止。")
                        .title("确认退出")
                        .kind(MessageDialogKind::Warning)
                        .buttons(MessageDialogButtons::OkCancelCustom(
                            "退出".into(),
                            "取消".into(),
                        ))
                        .parent(window)
                        .show(move |confirmed| {
                            if confirmed {
                                app.exit(0);
                            }
                        });
                }
                tauri::WindowEvent::Resized(_) if window.is_minimized().unwrap_or(false) => {
                    let _ = window.hide();
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            save_server,
            remove_server,
            save_sort_order,
            connect_server,
            disconnect_server,
            clear_server_fingerprint,
            save_service,
            remove_service,
            start_service,
            start_server_services,
            stop_server_services,
            stop_service,
            save_settings,
            open_terminal,
            terminal_input,
            terminal_resize,
            close_terminal,
            import_ssh_config,
            import_app_config,
            export_app_config,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build SSHGate");

    app.run(|app, event| {
        if matches!(event, tauri::RunEvent::Exit) {
            if let Some(state) = app.try_state::<AppState>() {
                tauri::async_runtime::block_on(state.shutdown());
            }
        }
    });
}
