<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { open as openFileDialog, save as saveFileDialog } from '@tauri-apps/plugin-dialog'
import { openUrl } from '@tauri-apps/plugin-opener'
import { ElMessage, ElMessageBox, type FormInstance, type FormRules, type TagProps } from 'element-plus'
import {
  CirclePlus, ExternalLink, FileInput, FileOutput, Fingerprint, Import, Monitor, Pencil, Plus, RefreshCw,
  Server, Settings as Setting, TerminalSquare, Trash2,
} from 'lucide-vue-next'
import { api } from './api'
import TerminalPane from './components/TerminalPane.vue'
import type { RuntimeSnapshot, Settings, SshServer, WebService } from './types'
import { showError } from './utils/errorDialog'

type Page = 'servers' | 'terminal' | 'fingerprints' | 'settings'
interface TerminalTab { id: string; serverId: string; title: string; password?: string }
type ServerForm = Omit<SshServer, 'port'> & { port?: number }
type ServiceForm = Omit<WebService, 'remotePort'> & { remotePort?: number }

const DEFAULT_SERVER_PORT = 22
const DEFAULT_PRIVATE_KEY_PATH = '~/.ssh/id_ed25519'
const DEFAULT_REMOTE_HOST = '127.0.0.1'
const DEFAULT_REMOTE_PORT = 3000

const page = ref<Page>('servers')
const snapshot = ref<RuntimeSnapshot>()
const loading = ref(true)
const serverModal = ref(false)
const serviceModal = ref(false)
const serverEditing = ref(false)
const serviceEditing = ref(false)
const serverFormRef = ref<FormInstance>()
const serviceFormRef = ref<FormInstance>()
const serviceDomainPrefix = ref('')
const serverSecret = ref('')
const originalServerAuthType = ref<SshServer['authType']>('key')
const originalRememberSecret = ref(false)
const passwordByServer = reactive<Record<string, string>>({})
const terminalTabs = ref<TerminalTab[]>([])
const activeTerminalId = ref('')
let unlistenState: UnlistenFn | undefined
let settingsSaveTimer: number | undefined
let settingsRevision = 0
let settingsSyncing = false

const blankServer = (): ServerForm => ({
  id: crypto.randomUUID(), name: '', host: '', port: undefined, username: '', authType: 'key', privateKeyPath: '', rememberSecret: false, hostKeyFingerprint: null,
})
const blankService = (serverId = ''): ServiceForm => ({
  id: crypto.randomUUID(), serverId, name: '', remoteHost: '', remotePort: undefined, domain: '', desiredRunning: false,
})
const serverForm = reactive<ServerForm>(blankServer())
const serviceForm = reactive<ServiceForm>(blankService())
const settingsForm = reactive<Settings>({ listenAddress: '127.0.0.1', listenPort: 80, reconnectDelaySeconds: 3, autoStartServices: true })

const serverRules: FormRules = {
  name: [{ required: true, message: '请输入服务器名称', trigger: 'blur' }],
  host: [{ required: true, message: '请输入主机地址', trigger: 'blur' }],
  username: [{ required: true, message: '请输入用户名', trigger: 'blur' }],
}
const serviceRules: FormRules = {
  name: [{ required: true, message: '请输入应用名称', trigger: 'blur' }],
  serverId: [{ required: true, message: '请选择 SSH 服务器', trigger: 'change' }],
}

const servers = computed(() => snapshot.value?.config.servers ?? [])
const services = computed(() => snapshot.value?.config.services ?? [])
const proxyHealthy = computed(() => !snapshot.value?.proxyError)

function syncSettingsForm(settings: Settings) {
  window.clearTimeout(settingsSaveTimer)
  settingsRevision += 1
  settingsSyncing = true
  Object.assign(settingsForm, settings)
  settingsSyncing = false
}

watch(settingsForm, () => {
  if (settingsSyncing || loading.value) return
  window.clearTimeout(settingsSaveTimer)
  const revision = ++settingsRevision
  settingsSaveTimer = window.setTimeout(async () => {
    const settings = { ...settingsForm }
    if (!settings.listenAddress.trim() || !settings.listenPort || !settings.reconnectDelaySeconds) {
      if (revision === settingsRevision) ElMessage.warning('设置未保存，请填写有效的监听地址、端口和重连间隔')
      return
    }
    try {
      const result = await api.saveSettings(settings)
      if (revision !== settingsRevision) return
      snapshot.value = result
      ElMessage.success('设置已自动保存')
    } catch (error) {
      if (revision === settingsRevision) await showError(error)
    }
  }, 600)
}, { deep: true, flush: 'sync' })

function normalizeDomainLabel(value: string, fallback = '') {
  return value.trim().toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '') || fallback
}
function normalizeDomainPrefix(value: string) {
  return value
    .trim()
    .toLowerCase()
    .replace(/^https?:\/\//, '')
    .replace(/\.localhost\.?$/, '')
    .split('.')
    .map((label) => normalizeDomainLabel(label))
    .filter(Boolean)
    .join('.')
}
function serverById(id: string) { return servers.value.find((server) => server.id === id) }
function servicesFor(id: string) { return services.value.filter((service) => service.serverId === id) }
function serverState(id: string) { return snapshot.value?.serverStates[id] ?? { status: 'stopped' as const } }
function serviceState(id: string) { return snapshot.value?.serviceStates[id] ?? { status: 'stopped' as const } }
function allServerAppsEnabled(serverId: string) {
  const apps = servicesFor(serverId)
  return apps.length > 0 && apps.every((app) => app.desiredRunning)
}
function serverAppsStarting(serverId: string) {
  return servicesFor(serverId).some((app) => ['starting', 'reconnecting'].includes(serviceState(app.id).status))
}
function stateLabel(status: string) {
  return ({ stopped: '已停止', connecting: '连接中', connected: '已连接', starting: '启动中', running: '运行中', error: '错误', reconnecting: '重连中' } as Record<string, string>)[status] || status
}
function stateType(status: string): TagProps['type'] {
  if (status === 'connected' || status === 'running') return 'success'
  if (status === 'connecting' || status === 'starting' || status === 'reconnecting') return 'warning'
  if (status === 'error') return 'danger'
  return 'info'
}

async function refresh() {
  try {
    snapshot.value = await api.snapshot()
    syncSettingsForm(snapshot.value.config.settings)
  } catch (error) { await showError(error) }
  finally { loading.value = false }
}
function showAddServer() {
  Object.assign(serverForm, blankServer())
  serverSecret.value = ''
  originalServerAuthType.value = 'key'
  originalRememberSecret.value = false
  serverEditing.value = false
  serverModal.value = true
  nextTick(() => serverFormRef.value?.clearValidate())
}
function showEditServer(server: SshServer) {
  Object.assign(serverForm, server, {
    port: server.port === DEFAULT_SERVER_PORT ? undefined : server.port,
    privateKeyPath: server.privateKeyPath === DEFAULT_PRIVATE_KEY_PATH ? '' : server.privateKeyPath,
  })
  serverSecret.value = ''
  originalServerAuthType.value = server.authType
  originalRememberSecret.value = server.rememberSecret
  serverEditing.value = true
  serverModal.value = true
  nextTick(() => serverFormRef.value?.clearValidate())
}
function serverAuthChanged() { serverSecret.value = '' }
function clearServerSecret() { serverSecret.value = '' }
function defaultDomainPrefix() {
  const server = serverById(serviceForm.serverId)
  return `${normalizeDomainLabel(serviceForm.name, 'service')}.${normalizeDomainLabel(server?.name ?? '', 'server')}`
}
function effectiveServiceDomain() { return `${normalizeDomainPrefix(serviceDomainPrefix.value) || defaultDomainPrefix()}.localhost` }
function effectiveRemoteHost() { return serviceForm.remoteHost.trim() || DEFAULT_REMOTE_HOST }
function effectiveRemotePort() { return serviceForm.remotePort ?? DEFAULT_REMOTE_PORT }
function showAddService(serverId = servers.value[0]?.id ?? '') { Object.assign(serviceForm, blankService(serverId)); serviceDomainPrefix.value = ''; serviceEditing.value = false; serviceModal.value = true; nextTick(() => serviceFormRef.value?.clearValidate()) }
function showEditService(service: WebService) { Object.assign(serviceForm, service, { remoteHost: service.remoteHost === DEFAULT_REMOTE_HOST ? '' : service.remoteHost, remotePort: service.remotePort === DEFAULT_REMOTE_PORT ? undefined : service.remotePort }); serviceDomainPrefix.value = service.domain.replace(/\.localhost\.?$/i, ''); serviceEditing.value = true; serviceModal.value = true; nextTick(() => serviceFormRef.value?.clearValidate()) }

async function run(action: () => Promise<RuntimeSnapshot | void>, success?: string) {
  try {
    const result = await action()
    if (result) snapshot.value = result as RuntimeSnapshot
    if (success) ElMessage.success(success)
    return true
  } catch (error) {
    await showError(error)
    return false
  }
}
async function submitServer() {
  if (!await serverFormRef.value?.validate().catch(() => false)) return
  const serverId = serverForm.id
  const temporarySecret = serverSecret.value
  const resolvedServer: SshServer = {
    ...serverForm,
    port: serverForm.port ?? DEFAULT_SERVER_PORT,
    privateKeyPath: serverForm.authType === 'key'
      ? serverForm.privateKeyPath.trim() || DEFAULT_PRIVATE_KEY_PATH
      : serverForm.privateKeyPath.trim(),
  }
  const needsNewSecret = serverForm.rememberSecret
    && !temporarySecret
    && (!serverEditing.value || !originalRememberSecret.value || originalServerAuthType.value !== serverForm.authType)
  if (needsNewSecret) {
    ElMessage.warning(`请输入要保存的${serverForm.authType === 'password' ? '密码' : '私钥口令'}`)
    return
  }
  if (await run(() => api.saveServer(resolvedServer, temporarySecret), '服务器已保存')) {
    if (temporarySecret && !serverForm.rememberSecret) passwordByServer[serverId] = temporarySecret
    else delete passwordByServer[serverId]
    serverSecret.value = ''
    serverModal.value = false
  }
}
async function submitService() {
  if (!await serviceFormRef.value?.validate().catch(() => false)) return
  const resolvedService: WebService = {
    ...serviceForm,
    remoteHost: effectiveRemoteHost(),
    remotePort: effectiveRemotePort(),
    domain: effectiveServiceDomain(),
  }
  if (await run(() => api.saveService(resolvedService), '应用已保存')) serviceModal.value = false
}
async function deleteServer(server: Pick<SshServer, 'id' | 'name'>) {
  try { await ElMessageBox.confirm(`删除“${server.name}”及其全部应用？`, '删除服务器', { type: 'warning', confirmButtonText: '删除', cancelButtonText: '取消' }) }
  catch { return }
  if (await run(() => api.removeServer(server.id), '服务器已删除')) serverModal.value = false
}
async function deleteService(service: Pick<WebService, 'id' | 'name'>) {
  try { await ElMessageBox.confirm(`确定删除应用“${service.name}”？`, '删除应用', { type: 'warning', confirmButtonText: '删除', cancelButtonText: '取消' }) }
  catch { return }
  if (await run(() => api.removeService(service.id), '应用已删除')) serviceModal.value = false
}
async function clearServerFingerprint(server: SshServer) {
  if (!server.hostKeyFingerprint) return
  try {
    await ElMessageBox.confirm(
      `清除“${server.name}”当前保存的主机指纹？下次连接时将重新信任并保存服务器提供的新指纹。`,
      '清除主机指纹',
      { type: 'warning', confirmButtonText: '清除', cancelButtonText: '取消' },
    )
  } catch { return }
  await run(() => api.clearServerFingerprint(server.id), '主机指纹已清除')
}
async function toggleService(service: WebService, enabled: boolean) {
  if (!enabled) {
    await run(() => api.stopService(service.id), '应用已停止')
    return
  }
  const server = serverById(service.serverId)
  if (!server) return
  const secret = await ensureServerConnection(server)
  if (secret === null) return
  await run(() => api.startService(service.id, secret), '应用已启动')
}
function serviceUrl(service: WebService) {
  const port = snapshot.value?.config.settings.listenPort ?? 80
  return `http://${service.domain}${port === 80 ? '' : `:${port}`}`
}
async function openService(service: WebService) { await openUrl(serviceUrl(service)) }
async function copyDomain(service: WebService) { await navigator.clipboard.writeText(serviceUrl(service)); ElMessage.success('访问地址已复制') }
async function askConnectionSecret(server: SshServer, required: boolean) {
  try {
    const { value } = await ElMessageBox.prompt(
      server.authType === 'key' ? '该私钥已加密，请输入私钥口令。口令仅用于本次运行，不会保存。' : '请输入密码。密码仅用于本次运行，不会保存。',
      server.authType === 'key' ? '私钥口令' : '密码',
      {
        confirmButtonText: '连接', cancelButtonText: '取消', inputType: 'password',
        inputValidator: (input) => !required || Boolean(input) || '请输入密码',
      },
    )
    return value ?? ''
  } catch { return null }
}
async function ensureServerConnection(server: SshServer) {
  if (serverState(server.id).status === 'connected') return passwordByServer[server.id]
  let secret = passwordByServer[server.id] || ''
  if (server.authType === 'password' && !server.rememberSecret && !secret) {
    const entered = await askConnectionSecret(server, true)
    if (entered === null) return null
    secret = entered
  }
  try {
    snapshot.value = await api.connectServer(server.id, secret)
  } catch (error) {
    const message = String(error)
    if (!secret && (server.authType === 'password' || /encrypted|加密|口令|凭据/i.test(message))) {
      const entered = await askConnectionSecret(server, true)
      if (entered === null) return null
      secret = entered
      try { snapshot.value = await api.connectServer(server.id, secret) }
      catch (retryError) { await showError(retryError); return null }
    } else {
      await showError(message)
      return null
    }
  }
  passwordByServer[server.id] = secret
  return secret
}
async function openTerminal(server: SshServer) {
  const secret = await ensureServerConnection(server)
  if (secret === null) return
  const id = crypto.randomUUID()
  terminalTabs.value.push({ id, serverId: server.id, title: server.name, password: secret })
  activeTerminalId.value = id
  page.value = 'terminal'
}
async function toggleAllServerApps(server: SshServer, enabled: boolean) {
  if (!enabled) {
    await run(() => api.stopServerServices(server.id), `“${server.name}”的应用已全部停止`)
    return
  }
  const secret = await ensureServerConnection(server)
  if (secret === null) return
  await run(() => api.startServerServices(server.id, secret), `“${server.name}”的应用已全部启动`)
}
function closeTerminal(id: string) {
  const index = terminalTabs.value.findIndex((tab) => tab.id === id)
  terminalTabs.value = terminalTabs.value.filter((tab) => tab.id !== id)
  if (activeTerminalId.value === id) activeTerminalId.value = terminalTabs.value[Math.max(0, index - 1)]?.id ?? ''
}
async function importConfig() {
  try {
    await ElMessageBox.confirm(
      '将读取本机 ~/.ssh/config 并导入其中可用的 SSH 服务器。已存在的同名配置不会重复导入，是否继续？',
      '确认导入 SSH Config',
      { type: 'warning', confirmButtonText: '确认导入', cancelButtonText: '取消' },
    )
    const imported = await api.importSshConfig(); await refresh()
    imported.length ? ElMessage.success(`已导入 ${imported.length} 台服务器`) : ElMessage.info('没有发现可导入的 Host')
  } catch (error) {
    if (error !== 'cancel' && error !== 'close') await showError(error)
  }
}
async function importAppConfig() {
  const path = await openFileDialog({
    multiple: false,
    directory: false,
    filters: [{ name: 'SSHGate Config', extensions: ['json'] }],
  })
  if (typeof path !== 'string') return
  try {
    await ElMessageBox.confirm(
      '导入将替换当前的服务器、应用、设置和主机指纹，并关闭现有终端与 SSH 连接。密码和私钥口令不会从配置文件导入。是否继续？',
      '导入应用 Config',
      { type: 'warning', confirmButtonText: '导入', cancelButtonText: '取消' },
    )
  } catch { return }
  try {
    snapshot.value = await api.importAppConfig(path)
    terminalTabs.value = []
    activeTerminalId.value = ''
    syncSettingsForm(snapshot.value.config.settings)
    ElMessage.success('应用 Config 已导入')
  } catch (error) { await showError(error) }
}
async function exportAppConfig() {
  const path = await saveFileDialog({
    defaultPath: 'sshgate-config.json',
    filters: [{ name: 'SSHGate Config', extensions: ['json'] }],
  })
  if (!path) return
  try {
    await api.exportAppConfig(path)
    ElMessage.success('应用 Config 已导出')
  } catch (error) { await showError(error) }
}
onMounted(async () => {
  document.documentElement.classList.remove('dark')
  localStorage.removeItem('sshgate-theme')
  unlistenState = await listen<RuntimeSnapshot>('state-changed', ({ payload }) => { snapshot.value = payload })
  await refresh()
})
onBeforeUnmount(() => {
  window.clearTimeout(settingsSaveTimer)
  unlistenState?.()
})
</script>

<template>
  <el-container class="app-frame">
    <el-aside width="232px" class="app-sidebar">
      <div class="brand"><span class="brand-icon"><TerminalSquare :size="20" /></span><div><strong>SSHGate</strong><small>Secure local gateway</small></div></div>
      <el-menu :default-active="page" class="nav-menu" @select="page = $event as Page">
        <el-menu-item index="servers"><Server :size="18" /><span>连接</span></el-menu-item>
        <el-menu-item index="terminal"><TerminalSquare :size="18" /><span>终端</span><el-tag v-if="terminalTabs.length" class="nav-counter" type="primary" effect="light" size="small" round>{{ terminalTabs.length }}</el-tag></el-menu-item>
        <el-menu-item index="fingerprints"><Fingerprint :size="18" /><span>指纹</span></el-menu-item>
        <el-menu-item index="settings"><Setting :size="18" /><span>设置</span></el-menu-item>
      </el-menu>
      <div class="sidebar-footer">
        <el-card shadow="never" class="sidebar-control-card">
          <div class="proxy-line"><el-badge is-dot :type="proxyHealthy ? 'success' : 'danger'" /><div><b>本地代理</b><small>{{ snapshot?.proxyError || `${settingsForm.listenAddress}:${settingsForm.listenPort}` }}</small></div></div>
        </el-card>
      </div>
    </el-aside>

    <el-container class="content-shell">
      <el-header class="topbar">
        <el-breadcrumb separator="/"><el-breadcrumb-item>SSHGate</el-breadcrumb-item><el-breadcrumb-item>{{ { servers: '连接', terminal: '终端', fingerprints: '指纹', settings: '设置' }[page] }}</el-breadcrumb-item></el-breadcrumb>
        <el-button circle text :icon="RefreshCw" title="刷新状态" @click="refresh" />
      </el-header>
      <el-main :class="['page-content', { 'terminal-content': page === 'terminal' }]">
        <template v-if="page === 'servers'">
          <div class="page-heading"><div><h1>连接</h1><p>管理 SSH 服务器、远端应用和交互式终端。</p></div><el-button type="primary" :icon="Plus" @click="showAddServer">添加服务器</el-button></div>
          <el-skeleton :loading="loading" animated :rows="6">
            <div v-if="servers.length" class="server-grid">
              <el-card v-for="server in servers" :key="server.id" shadow="hover" class="server-card">
                <template #header><div class="server-header"><el-avatar shape="square" :size="44"><Server :size="22" /></el-avatar><div class="server-title"><h3>{{ server.name }}</h3><code class="server-address">{{ server.username }}@{{ server.host }}:{{ server.port }}</code></div><el-tag :type="stateType(serverState(server.id).status)" effect="light" round>{{ stateLabel(serverState(server.id).status) }}</el-tag><div class="server-header-actions"><el-button text circle :icon="TerminalSquare" title="打开终端" aria-label="打开终端" @click="openTerminal(server)" /><el-button text circle :icon="CirclePlus" title="添加应用" aria-label="添加应用" @click="showAddService(server.id)" /><el-button text circle title="编辑服务器" aria-label="编辑服务器" @click="showEditServer(server)"><Setting :size="16" /></el-button><el-switch v-if="servicesFor(server.id).length" :model-value="allServerAppsEnabled(server.id)" :loading="serverAppsStarting(server.id)" :title="allServerAppsEnabled(server.id) ? '停止全部应用' : '启动全部应用'" :aria-label="allServerAppsEnabled(server.id) ? '停止全部应用' : '启动全部应用'" @change="toggleAllServerApps(server, Boolean($event))" /></div></div></template>
                <el-input v-if="server.authType === 'password' && serverState(server.id).status !== 'connected' && !server.rememberSecret" v-model="passwordByServer[server.id]" class="password-input" type="password" show-password />
                <el-empty v-if="!servicesFor(server.id).length" :image-size="46" description="尚未添加应用" />
                <el-table v-else :data="servicesFor(server.id)" size="small" :show-header="false" class="embedded-table">
                  <el-table-column min-width="170"><template #default="{ row }"><div class="service-name"><b>{{ row.name }}</b><el-link type="primary" :underline="false" @click="copyDomain(row)">{{ row.domain }}</el-link></div></template></el-table-column>
                  <el-table-column width="125"><template #default="{ row }"><el-text type="info"><code>{{ row.remoteHost }}:{{ row.remotePort }}</code></el-text></template></el-table-column>
                  <el-table-column width="132" align="right"><template #default="{ row }"><el-space :size="4"><el-button circle text :icon="ExternalLink" title="打开应用" :disabled="serviceState(row.id).status !== 'running'" @click="openService(row)" /><el-button circle text :icon="Pencil" title="编辑应用" @click="showEditService(row)" /><el-switch :model-value="row.desiredRunning" :loading="['starting', 'reconnecting'].includes(serviceState(row.id).status)" @change="toggleService(row, Boolean($event))" /></el-space></template></el-table-column>
                </el-table>
              </el-card>
            </div>
            <el-empty v-else description="还没有 SSH 服务器"><el-button type="primary" :icon="Plus" @click="showAddServer">添加第一台服务器</el-button></el-empty>
          </el-skeleton>
        </template>

        <div v-show="page === 'terminal'" class="terminal-workspace">
          <el-tabs v-if="terminalTabs.length" v-model="activeTerminalId" type="card" closable class="terminal-tabs" @tab-remove="closeTerminal(String($event))">
            <el-tab-pane v-for="tab in terminalTabs" :key="tab.id" :name="tab.id" :label="tab.title"><TerminalPane :terminal-id="tab.id" :server-id="tab.serverId" :password="tab.password" @closed="closeTerminal(tab.id)" /></el-tab-pane>
          </el-tabs>
          <el-empty v-if="!terminalTabs.length" description="请从连接页选择服务器打开终端" />
        </div>

        <template v-if="page === 'fingerprints'">
          <div class="page-heading"><div><h1>指纹</h1><p>查看和管理 SSH 服务器当前保存的主机密钥指纹。</p></div></div>
          <el-card shadow="never" class="fingerprint-card">
            <el-table v-if="servers.length" :data="servers" class="fingerprint-table">
              <el-table-column label="服务器" width="220"><template #default="{ row }"><div class="fingerprint-server"><b>{{ row.name }}</b><code>{{ row.host }}:{{ row.port }}</code></div></template></el-table-column>
              <el-table-column label="已保存指纹" min-width="80"><template #default="{ row }"><code v-if="row.hostKeyFingerprint" class="fingerprint-value" :title="row.hostKeyFingerprint">{{ row.hostKeyFingerprint }}</code><el-text v-else type="info">尚未记录</el-text></template></el-table-column>
              <el-table-column width="96" align="right" fixed="right"><template #default="{ row }"><el-button link type="danger" :icon="Trash2" :disabled="!row.hostKeyFingerprint" @click="clearServerFingerprint(row)">清除</el-button></template></el-table-column>
            </el-table>
            <el-empty v-else description="还没有 SSH 服务器" />
          </el-card>
        </template>

        <template v-if="page === 'settings'">
          <div class="page-heading"><div><h1>设置</h1><p>调整本地代理入口和连接恢复行为。</p></div></div>
          <el-card shadow="never" class="settings-card">
            <el-form :model="settingsForm" label-position="top">
              <h3>HTTP 反向代理</h3><el-text type="info">端口 80 可直接使用 http://*.localhost；端口被占用时可在这里修改。</el-text>
              <div class="settings-row"><el-form-item class="settings-address-field" label="监听地址"><el-input v-model="settingsForm.listenAddress" /></el-form-item><el-form-item class="settings-number-field" label="监听端口"><el-input-number v-model="settingsForm.listenPort" class="port-input" :min="1" :max="65535" :controls="false" align="left" /></el-form-item></div>
              <el-divider />
              <h3>连接恢复</h3><div class="settings-row"><el-form-item class="settings-number-field" label="重连间隔（秒）"><el-input-number v-model="settingsForm.reconnectDelaySeconds" :min="1" :max="300" controls-position="right" /></el-form-item><el-form-item class="settings-switch-field" label="启动时恢复应用"><el-switch v-model="settingsForm.autoStartServices" /></el-form-item></div>
              <el-divider />
              <h3>配置管理</h3><el-text type="info">应用 Config 包含服务器、应用、设置和主机指纹，不包含密码或私钥口令。</el-text>
              <div class="config-actions"><el-button :icon="Import" @click="importConfig">导入 SSH Config</el-button><el-button :icon="FileInput" @click="importAppConfig">导入应用 Config</el-button><el-button :icon="FileOutput" @click="exportAppConfig">导出应用 Config</el-button></div>
            </el-form>
          </el-card>
        </template>
      </el-main>
    </el-container>

    <el-dialog v-model="serverModal" :title="serverEditing ? '编辑服务器' : '添加服务器'" width="560px" class="server-dialog" align-center destroy-on-close @closed="clearServerSecret">
      <el-form ref="serverFormRef" :model="serverForm" :rules="serverRules" label-position="top">
        <el-form-item label="名称" prop="name"><el-input v-model="serverForm.name" /></el-form-item>
        <el-row :gutter="16"><el-col :span="18"><el-form-item label="主机" prop="host"><el-input v-model="serverForm.host" /></el-form-item></el-col><el-col :span="6"><el-form-item label="端口"><el-input-number v-model="serverForm.port" class="port-input" :min="1" :max="65535" :controls="false" :placeholder="String(DEFAULT_SERVER_PORT)" align="left" /></el-form-item></el-col></el-row>
        <el-row :gutter="16"><el-col :span="18"><el-form-item label="用户名" prop="username"><el-input v-model="serverForm.username" /></el-form-item></el-col><el-col :span="6"><el-form-item label="认证方式"><el-select v-model="serverForm.authType" @change="serverAuthChanged"><el-option label="密钥" value="key" /><el-option label="密码" value="password" /></el-select></el-form-item></el-col></el-row>
        <el-form-item v-if="serverForm.authType === 'key'" label="私钥路径"><el-input v-model="serverForm.privateKeyPath" :placeholder="DEFAULT_PRIVATE_KEY_PATH" /><div class="form-help">仅保存路径，不复制私钥内容。</div></el-form-item>
        <el-form-item class="secret-form-item" :label="serverForm.authType === 'password' ? '密码' : '私钥口令'"><el-input v-model="serverSecret" type="password" show-password autocomplete="new-password" /><div class="secret-options"><el-checkbox v-model="serverForm.rememberSecret">保存到系统凭据库</el-checkbox><span>配置文件不会保存明文</span></div></el-form-item>
      </el-form>
      <template #footer><div class="dialog-footer"><el-button v-if="serverEditing" type="danger" plain :icon="Trash2" @click="deleteServer(serverForm)">删除</el-button><span /><el-button @click="serverModal = false">取消</el-button><el-button type="primary" @click="submitServer">保存</el-button></div></template>
    </el-dialog>

    <el-dialog v-model="serviceModal" :title="serviceEditing ? '编辑应用' : '添加应用'" width="560px" align-center destroy-on-close>
      <el-form ref="serviceFormRef" :model="serviceForm" :rules="serviceRules" label-position="top">
        <el-row :gutter="16"><el-col :span="18"><el-form-item label="名称" prop="name"><el-input v-model="serviceForm.name" /></el-form-item></el-col><el-col :span="6"><el-form-item label="SSH 服务器" prop="serverId"><el-select v-model="serviceForm.serverId"><el-option v-for="server in servers" :key="server.id" :label="server.name" :value="server.id" /></el-select></el-form-item></el-col></el-row>
        <el-row :gutter="16"><el-col :span="18"><el-form-item label="远端主机"><el-input v-model="serviceForm.remoteHost" :placeholder="DEFAULT_REMOTE_HOST" /></el-form-item></el-col><el-col :span="6"><el-form-item label="远端端口"><el-input-number v-model="serviceForm.remotePort" class="port-input" :min="1" :max="65535" :controls="false" :placeholder="String(DEFAULT_REMOTE_PORT)" align="left" /></el-form-item></el-col></el-row>
        <el-form-item label="访问域名"><el-input v-model="serviceDomainPrefix" :placeholder="defaultDomainPrefix()"><template #prepend>http://</template><template #append>.localhost</template></el-input><div class="form-help">可留空，默认使用“应用名.服务器名.localhost”；保存时会自动小写并规范化字符。</div></el-form-item>
        <el-alert type="info" :closable="false" show-icon><template #title><span class="route-summary"><Monitor :size="14" />浏览器 → <code>{{ effectiveServiceDomain() }}</code> → SSH → <code>{{ effectiveRemoteHost() }}:{{ effectiveRemotePort() }}</code></span></template></el-alert>
      </el-form>
      <template #footer><div class="dialog-footer"><el-button v-if="serviceEditing" type="danger" plain :icon="Trash2" @click="deleteService(serviceForm)">删除</el-button><span /><el-button @click="serviceModal = false">取消</el-button><el-button type="primary" @click="submitService">保存</el-button></div></template>
    </el-dialog>
  </el-container>
</template>
