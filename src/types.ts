export type ConnectionStatus = 'stopped' | 'connecting' | 'connected' | 'error' | 'reconnecting'
export type ServiceStatus = 'stopped' | 'starting' | 'running' | 'error' | 'reconnecting'

export interface SshServer {
  id: string
  name: string
  host: string
  port: number
  username: string
  authType: 'key' | 'password'
  privateKeyPath: string
  rememberSecret: boolean
  hostKeyFingerprint?: string | null
}

export interface WebService {
  id: string
  serverId: string
  name: string
  serviceType: 'http' | 'tcp'
  remoteHost: string
  remotePort: number
  localAddress: string
  localPort: number
  domain: string
  desiredRunning: boolean
}

export interface Settings {
  listenAddress: string
  listenPort: number
  reconnectDelaySeconds: number
  autoStartServices: boolean
  privacyMode: boolean
}

export interface AppConfig {
  servers: SshServer[]
  services: WebService[]
  settings: Settings
}

export interface RuntimeSnapshot {
  config: AppConfig
  serverStates: Record<string, { status: ConnectionStatus; error?: string | null }>
  serviceStates: Record<string, { status: ServiceStatus; error?: string | null }>
  proxyError?: string | null
}

export interface TerminalEvent {
  terminalId: string
  data?: string
  message?: string
  exitStatus?: number
}
