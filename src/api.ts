import { invoke } from '@tauri-apps/api/core'
import type { RuntimeSnapshot, Settings, SshServer, WebService } from './types'

export const api = {
  snapshot: () => invoke<RuntimeSnapshot>('get_snapshot'),
  saveServer: (server: SshServer, secret?: string) =>
    invoke<RuntimeSnapshot>('save_server', { server, secret: secret || null }),
  removeServer: (serverId: string) => invoke<RuntimeSnapshot>('remove_server', { serverId }),
  connectServer: (serverId: string, password?: string) =>
    invoke<RuntimeSnapshot>('connect_server', { serverId, password: password || null }),
  disconnectServer: (serverId: string) => invoke<RuntimeSnapshot>('disconnect_server', { serverId }),
  clearServerFingerprint: (serverId: string) => invoke<RuntimeSnapshot>('clear_server_fingerprint', { serverId }),
  saveService: (service: WebService) => invoke<RuntimeSnapshot>('save_service', { service }),
  removeService: (serviceId: string) => invoke<RuntimeSnapshot>('remove_service', { serviceId }),
  startService: (serviceId: string, password?: string) =>
    invoke<RuntimeSnapshot>('start_service', { serviceId, password: password || null }),
  startServerServices: (serverId: string, password?: string) =>
    invoke<RuntimeSnapshot>('start_server_services', { serverId, password: password || null }),
  stopServerServices: (serverId: string) =>
    invoke<RuntimeSnapshot>('stop_server_services', { serverId }),
  stopService: (serviceId: string) => invoke<RuntimeSnapshot>('stop_service', { serviceId }),
  saveSettings: (settings: Settings) => invoke<RuntimeSnapshot>('save_settings', { settings }),
  openTerminal: (serverId: string, terminalId: string, cols: number, rows: number, password?: string) =>
    invoke<void>('open_terminal', { serverId, terminalId, cols, rows, password: password || null }),
  terminalInput: (terminalId: string, data: string) => invoke<void>('terminal_input', { terminalId, data }),
  terminalResize: (terminalId: string, cols: number, rows: number) =>
    invoke<void>('terminal_resize', { terminalId, cols, rows }),
  closeTerminal: (terminalId: string) => invoke<void>('close_terminal', { terminalId }),
  importSshConfig: () => invoke<SshServer[]>('import_ssh_config'),
}
