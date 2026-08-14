<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { getVersion } from '@tauri-apps/api/app'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { open as openFileDialog, save as saveFileDialog } from '@tauri-apps/plugin-dialog'
import { openUrl } from '@tauri-apps/plugin-opener'
import { ElMessage, ElMessageBox, type FormInstance, type FormRules, type TagProps } from 'element-plus'
import { useI18n } from 'vue-i18n'
import {
  ArrowDown, ArrowUp, ArrowUpDown, Check, CirclePlus, ExternalLink, FileInput, FileOutput, Fingerprint, Import, Monitor, Pencil, Plus, RefreshCw,
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

const { t, te } = useI18n()

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
const appVersion = ref('')
const sortMode = ref(false)
const sortSaving = ref(false)
const sortedServerIds = ref<string[]>([])
const sortedServiceIds = reactive<Record<string, string[]>>({})
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

const serverRules = computed<FormRules>(() => ({
  name: [{ required: true, message: t('validation.serverName'), trigger: 'blur' }],
  host: [{ required: true, message: t('validation.host'), trigger: 'blur' }],
  username: [{ required: true, message: t('validation.username'), trigger: 'blur' }],
}))
const serviceRules = computed<FormRules>(() => ({
  name: [{ required: true, message: t('validation.serviceName'), trigger: 'blur' }],
  serverId: [{ required: true, message: t('validation.server'), trigger: 'change' }],
}))

const servers = computed(() => snapshot.value?.config.servers ?? [])
const services = computed(() => snapshot.value?.config.services ?? [])
const proxyHealthy = computed(() => !snapshot.value?.proxyError)
const runningServiceCount = computed(() => services.value.filter((service) => serviceState(service.id).status === 'running').length)

function orderByIds<T extends { id: string }>(items: T[], ids: string[]) {
  const itemsById = new Map(items.map((item) => [item.id, item]))
  return ids.map((id) => itemsById.get(id)).filter((item): item is T => item !== undefined)
}

const displayedServers = computed(() => sortMode.value
  ? orderByIds(servers.value, sortedServerIds.value)
  : servers.value)

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
      if (revision === settingsRevision) ElMessage.warning(t('settings.invalid'))
      return
    }
    try {
      const result = await api.saveSettings(settings)
      if (revision !== settingsRevision) return
      snapshot.value = result
      ElMessage.success(t('settings.autoSaved'))
    } catch (error) {
      if (revision === settingsRevision) await showError(error)
    }
  }, 600)
}, { deep: true, flush: 'sync' })

function normalizeDomainLabel(value: string, fallback = '') {
  return value.trim().toLowerCase().replace(/[^\p{L}\p{N}]+/gu, '-').replace(/^-+|-+$/g, '') || fallback
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
function displayedServicesFor(id: string) {
  const serverServices = servicesFor(id)
  return sortMode.value ? orderByIds(serverServices, sortedServiceIds[id] ?? []) : serverServices
}
function moveItem<T>(items: T[], index: number, offset: -1 | 1) {
  const target = index + offset
  if (target < 0 || target >= items.length) return
  const [item] = items.splice(index, 1)
  items.splice(target, 0, item)
}
function beginSorting() {
  sortedServerIds.value = servers.value.map((server) => server.id)
  Object.keys(sortedServiceIds).forEach((id) => delete sortedServiceIds[id])
  servers.value.forEach((server) => {
    sortedServiceIds[server.id] = servicesFor(server.id).map((service) => service.id)
  })
  sortMode.value = true
}
async function finishSorting() {
  if (sortSaving.value) return
  sortSaving.value = true
  try {
    const serviceIds = sortedServerIds.value.flatMap((serverId) => sortedServiceIds[serverId] ?? [])
    snapshot.value = await api.saveSortOrder([...sortedServerIds.value], serviceIds)
    sortMode.value = false
    ElMessage.success(t('messages.orderSaved'))
  } catch (error) {
    await showError(error)
  } finally {
    sortSaving.value = false
  }
}
async function toggleSorting() {
  if (sortMode.value) await finishSorting()
  else beginSorting()
}
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
  const key = `status.${status}`
  return te(key) ? t(key) : status
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
    ElMessage.warning(t('secret.saveRequired', { type: t(serverForm.authType === 'password' ? 'serverDialog.password' : 'serverDialog.passphrase') }))
    return
  }
  if (await run(() => api.saveServer(resolvedServer, temporarySecret), t('messages.serverSaved'))) {
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
  if (await run(() => api.saveService(resolvedService), t('messages.serviceSaved'))) serviceModal.value = false
}
async function deleteServer(server: Pick<SshServer, 'id' | 'name'>) {
  try { await ElMessageBox.confirm(t('confirmations.deleteServer', { name: server.name }), t('confirmations.deleteServerTitle'), { type: 'warning', confirmButtonText: t('common.delete'), cancelButtonText: t('common.cancel') }) }
  catch { return }
  if (await run(() => api.removeServer(server.id), t('messages.serverDeleted'))) serverModal.value = false
}
async function deleteService(service: Pick<WebService, 'id' | 'name'>) {
  try { await ElMessageBox.confirm(t('confirmations.deleteService', { name: service.name }), t('confirmations.deleteServiceTitle'), { type: 'warning', confirmButtonText: t('common.delete'), cancelButtonText: t('common.cancel') }) }
  catch { return }
  if (await run(() => api.removeService(service.id), t('messages.serviceDeleted'))) serviceModal.value = false
}
async function clearServerFingerprint(server: SshServer) {
  if (!server.hostKeyFingerprint) return
  try {
    await ElMessageBox.confirm(
      t('fingerprints.confirmMessage', { name: server.name }),
      t('fingerprints.confirmTitle'),
      { type: 'warning', confirmButtonText: t('common.clear'), cancelButtonText: t('common.cancel') },
    )
  } catch { return }
  await run(() => api.clearServerFingerprint(server.id), t('fingerprints.cleared'))
}
async function toggleService(service: WebService, enabled: boolean) {
  if (!enabled) {
    await run(() => api.stopService(service.id), t('messages.serviceStopped'))
    return
  }
  const server = serverById(service.serverId)
  if (!server) return
  const secret = await ensureServerConnection(server)
  if (secret === null) return
  await run(() => api.startService(service.id, secret), t('messages.serviceStarted'))
}
function serviceUrl(service: WebService) {
  const port = snapshot.value?.config.settings.listenPort ?? 80
  return `http://${service.domain}${port === 80 ? '' : `:${port}`}`
}
async function openService(service: WebService) { await openUrl(serviceUrl(service)) }
async function copyDomain(service: WebService) { await navigator.clipboard.writeText(serviceUrl(service)); ElMessage.success(t('messages.addressCopied')) }
async function askConnectionSecret(server: SshServer, required: boolean) {
  try {
    const { value } = await ElMessageBox.prompt(
      t(server.authType === 'key' ? 'secret.encryptedKeyPrompt' : 'secret.passwordPrompt'),
      t(server.authType === 'key' ? 'serverDialog.passphrase' : 'serverDialog.password'),
      {
        confirmButtonText: t('common.connect'), cancelButtonText: t('common.cancel'), inputType: 'password',
        inputValidator: (input) => !required || Boolean(input) || t('secret.passwordRequired'),
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
    if (!secret && (server.authType === 'password' || new RegExp(t('secret.connectionErrorPattern'), 'i').test(message))) {
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
    await run(() => api.stopServerServices(server.id), t('messages.allServicesStopped', { name: server.name }))
    return
  }
  const secret = await ensureServerConnection(server)
  if (secret === null) return
  await run(() => api.startServerServices(server.id, secret), t('messages.allServicesStarted', { name: server.name }))
}
function closeTerminal(id: string) {
  const index = terminalTabs.value.findIndex((tab) => tab.id === id)
  terminalTabs.value = terminalTabs.value.filter((tab) => tab.id !== id)
  if (activeTerminalId.value === id) activeTerminalId.value = terminalTabs.value[Math.max(0, index - 1)]?.id ?? ''
}
async function importConfig() {
  try {
    await ElMessageBox.confirm(
      t('config.importSshMessage'),
      t('config.importSshTitle'),
      { type: 'warning', confirmButtonText: t('config.confirmImport'), cancelButtonText: t('common.cancel') },
    )
    const imported = await api.importSshConfig(); await refresh()
    imported.length ? ElMessage.success(t('config.importedServers', { count: imported.length })) : ElMessage.info(t('config.noImportableHosts'))
  } catch (error) {
    if (error !== 'cancel' && error !== 'close') await showError(error)
  }
}
async function importAppConfig() {
  const path = await openFileDialog({
    multiple: false,
    directory: false,
    filters: [{ name: t('config.fileType'), extensions: ['json'] }],
  })
  if (typeof path !== 'string') return
  try {
    await ElMessageBox.confirm(
      t('config.importAppMessage'),
      t('config.importAppTitle'),
      { type: 'warning', confirmButtonText: t('config.import'), cancelButtonText: t('common.cancel') },
    )
  } catch { return }
  try {
    snapshot.value = await api.importAppConfig(path)
    terminalTabs.value = []
    activeTerminalId.value = ''
    syncSettingsForm(snapshot.value.config.settings)
    ElMessage.success(t('config.appImported'))
  } catch (error) { await showError(error) }
}
async function exportAppConfig() {
  const path = await saveFileDialog({
    defaultPath: 'sshgate-config.json',
    filters: [{ name: t('config.fileType'), extensions: ['json'] }],
  })
  if (!path) return
  try {
    await api.exportAppConfig(path)
    ElMessage.success(t('config.appExported'))
  } catch (error) { await showError(error) }
}
onMounted(async () => {
  document.documentElement.classList.remove('dark')
  localStorage.removeItem('sshgate-theme')
  const versionPromise = getVersion().catch(() => '')
  unlistenState = await listen<RuntimeSnapshot>('state-changed', ({ payload }) => { snapshot.value = payload })
  await refresh()
  appVersion.value = await versionPromise
})
onBeforeUnmount(() => {
  window.clearTimeout(settingsSaveTimer)
  unlistenState?.()
})
</script>

<template>
  <el-container class="app-frame">
    <el-aside width="232px" class="app-sidebar">
      <div class="brand"><span class="brand-icon"><TerminalSquare :size="20" /></span><div><strong>SSHGate</strong><small>{{ appVersion ? t('brand.version', { version: appVersion }) : '' }}</small></div></div>
      <el-menu :default-active="page" class="nav-menu" @select="page = $event as Page">
        <el-menu-item index="servers"><Server :size="18" /><span>{{ t('nav.connections') }}</span><el-tag v-if="runningServiceCount" class="nav-counter" type="success" effect="light" size="small" round>{{ runningServiceCount }}</el-tag></el-menu-item>
        <el-menu-item index="terminal"><TerminalSquare :size="18" /><span>{{ t('nav.terminal') }}</span><el-tag v-if="terminalTabs.length" class="nav-counter" type="primary" effect="light" size="small" round>{{ terminalTabs.length }}</el-tag></el-menu-item>
        <el-menu-item index="fingerprints"><Fingerprint :size="18" /><span>{{ t('nav.fingerprints') }}</span></el-menu-item>
        <el-menu-item index="settings"><Setting :size="18" /><span>{{ t('nav.settings') }}</span></el-menu-item>
      </el-menu>
      <div class="sidebar-footer">
        <el-card shadow="never" class="sidebar-control-card">
          <div class="proxy-line"><el-badge is-dot :type="proxyHealthy ? 'success' : 'danger'" /><div><b>{{ t('sidebar.localProxy') }}</b><small>{{ snapshot?.proxyError || `${settingsForm.listenAddress}:${settingsForm.listenPort}` }}</small></div></div>
        </el-card>
      </div>
    </el-aside>

    <el-container class="content-shell">
      <el-header class="topbar">
        <el-breadcrumb separator="/"><el-breadcrumb-item>SSHGate</el-breadcrumb-item><el-breadcrumb-item>{{ t(`nav.${page === 'servers' ? 'connections' : page}`) }}</el-breadcrumb-item></el-breadcrumb>
        <el-button circle text :icon="RefreshCw" :title="t('actions.refreshStatus')" @click="refresh" />
      </el-header>
      <el-main :class="['page-content', { 'terminal-content': page === 'terminal' }]">
        <template v-if="page === 'servers'">
          <div class="page-heading"><div><h1>{{ t('connections.title') }}</h1><p>{{ t('connections.description') }}</p></div><el-space><el-button :type="sortMode ? 'primary' : 'default'" :icon="sortMode ? Check : ArrowUpDown" :loading="sortSaving" @click="toggleSorting">{{ t(sortMode ? 'actions.finishSorting' : 'actions.sort') }}</el-button><el-button v-if="!sortMode" type="primary" :icon="Plus" @click="showAddServer">{{ t('actions.addServer') }}</el-button></el-space></div>
          <el-skeleton :loading="loading" animated :rows="6">
            <div v-if="displayedServers.length" class="server-grid">
              <el-card v-for="(server, serverIndex) in displayedServers" :key="server.id" shadow="hover" class="server-card">
                <template #header><div class="server-header"><el-avatar shape="square" :size="44"><Server :size="22" /></el-avatar><div class="server-title"><h3>{{ server.name }}</h3><code class="server-address">{{ server.username }}@{{ server.host }}:{{ server.port }}</code></div><el-tag :type="stateType(serverState(server.id).status)" effect="light" round>{{ stateLabel(serverState(server.id).status) }}</el-tag><div class="server-header-actions"><template v-if="sortMode"><el-button text circle :icon="ArrowUp" :title="t('actions.moveUp')" :aria-label="t('actions.moveUp')" :disabled="serverIndex === 0" @click="moveItem(sortedServerIds, serverIndex, -1)" /><el-button text circle :icon="ArrowDown" :title="t('actions.moveDown')" :aria-label="t('actions.moveDown')" :disabled="serverIndex === displayedServers.length - 1" @click="moveItem(sortedServerIds, serverIndex, 1)" /></template><template v-else><el-button text circle :icon="TerminalSquare" :title="t('actions.openTerminal')" :aria-label="t('actions.openTerminal')" @click="openTerminal(server)" /><el-button text circle :icon="CirclePlus" :title="t('actions.addService')" :aria-label="t('actions.addService')" @click="showAddService(server.id)" /><el-button text circle :title="t('actions.editServer')" :aria-label="t('actions.editServer')" @click="showEditServer(server)"><Setting :size="16" /></el-button><el-switch v-if="servicesFor(server.id).length" :model-value="allServerAppsEnabled(server.id)" :loading="serverAppsStarting(server.id)" :title="t(allServerAppsEnabled(server.id) ? 'actions.stopAllServices' : 'actions.startAllServices')" :aria-label="t(allServerAppsEnabled(server.id) ? 'actions.stopAllServices' : 'actions.startAllServices')" @change="toggleAllServerApps(server, Boolean($event))" /></template></div></div></template>
                <el-input v-if="server.authType === 'password' && serverState(server.id).status !== 'connected' && !server.rememberSecret" v-model="passwordByServer[server.id]" class="password-input" type="password" show-password />
                <el-empty v-if="!displayedServicesFor(server.id).length" :image-size="46" :description="t('connections.noServices')" />
                <el-table v-else :data="displayedServicesFor(server.id)" size="small" :show-header="false" class="embedded-table">
                  <el-table-column min-width="170"><template #default="{ row }"><div class="service-name"><b>{{ row.name }}</b><el-link type="primary" :underline="false" @click="copyDomain(row)">{{ row.domain }}</el-link></div></template></el-table-column>
                  <el-table-column width="125"><template #default="{ row }"><el-text type="info"><code>{{ row.remoteHost }}:{{ row.remotePort }}</code></el-text></template></el-table-column>
                  <el-table-column width="132" align="right"><template #default="{ row, $index }"><el-space :size="4"><template v-if="sortMode"><el-button circle text :icon="ArrowUp" :title="t('actions.moveUp')" :aria-label="t('actions.moveUp')" :disabled="$index === 0" @click="moveItem(sortedServiceIds[server.id], $index, -1)" /><el-button circle text :icon="ArrowDown" :title="t('actions.moveDown')" :aria-label="t('actions.moveDown')" :disabled="$index === displayedServicesFor(server.id).length - 1" @click="moveItem(sortedServiceIds[server.id], $index, 1)" /></template><template v-else><el-button circle text :icon="ExternalLink" :title="t('actions.openService')" :disabled="serviceState(row.id).status !== 'running'" @click="openService(row)" /><el-button circle text :icon="Pencil" :title="t('actions.editService')" @click="showEditService(row)" /><el-switch :model-value="row.desiredRunning" :loading="['starting', 'reconnecting'].includes(serviceState(row.id).status)" @change="toggleService(row, Boolean($event))" /></template></el-space></template></el-table-column>
                </el-table>
              </el-card>
            </div>
            <el-empty v-else :description="t('connections.noServers')"><el-button type="primary" :icon="Plus" @click="showAddServer">{{ t('connections.addFirstServer') }}</el-button></el-empty>
          </el-skeleton>
        </template>

        <div v-show="page === 'terminal'" class="terminal-workspace">
          <el-tabs v-if="terminalTabs.length" v-model="activeTerminalId" type="card" closable class="terminal-tabs" @tab-remove="closeTerminal(String($event))">
            <el-tab-pane v-for="tab in terminalTabs" :key="tab.id" :name="tab.id" :label="tab.title"><TerminalPane :terminal-id="tab.id" :server-id="tab.serverId" :password="tab.password" @closed="closeTerminal(tab.id)" /></el-tab-pane>
          </el-tabs>
          <el-empty v-if="!terminalTabs.length" :description="t('terminal.empty')" />
        </div>

        <template v-if="page === 'fingerprints'">
          <div class="page-heading"><div><h1>{{ t('fingerprints.title') }}</h1><p>{{ t('fingerprints.description') }}</p></div></div>
          <el-card shadow="never" class="fingerprint-card">
            <el-table v-if="servers.length" :data="servers" class="fingerprint-table">
              <el-table-column :label="t('fingerprints.server')" width="220"><template #default="{ row }"><div class="fingerprint-server"><b>{{ row.name }}</b><code>{{ row.host }}:{{ row.port }}</code></div></template></el-table-column>
              <el-table-column :label="t('fingerprints.savedFingerprint')" min-width="80"><template #default="{ row }"><code v-if="row.hostKeyFingerprint" class="fingerprint-value" :title="row.hostKeyFingerprint">{{ row.hostKeyFingerprint }}</code><el-text v-else type="info">{{ t('fingerprints.notRecorded') }}</el-text></template></el-table-column>
              <el-table-column width="96" align="right" fixed="right"><template #default="{ row }"><el-button link type="danger" :icon="Trash2" :disabled="!row.hostKeyFingerprint" @click="clearServerFingerprint(row)">{{ t('common.clear') }}</el-button></template></el-table-column>
            </el-table>
            <el-empty v-else :description="t('connections.noServers')" />
          </el-card>
        </template>

        <template v-if="page === 'settings'">
          <div class="page-heading"><div><h1>{{ t('settings.title') }}</h1><p>{{ t('settings.description') }}</p></div></div>
          <el-card shadow="never" class="settings-card">
            <el-form :model="settingsForm" label-position="top">
              <h3>{{ t('settings.proxyTitle') }}</h3><el-text type="info">{{ t('settings.proxyHelp') }}</el-text>
              <div class="settings-row"><el-form-item class="settings-address-field" :label="t('settings.listenAddress')"><el-input v-model="settingsForm.listenAddress" /></el-form-item><el-form-item class="settings-number-field" :label="t('settings.listenPort')"><el-input-number v-model="settingsForm.listenPort" class="port-input" :min="1" :max="65535" :controls="false" align="left" /></el-form-item></div>
              <el-divider />
              <h3>{{ t('settings.recoveryTitle') }}</h3><div class="settings-row"><el-form-item class="settings-number-field" :label="t('settings.reconnectDelay')"><el-input-number v-model="settingsForm.reconnectDelaySeconds" :min="1" :max="300" controls-position="right" /></el-form-item><el-form-item class="settings-switch-field" :label="t('settings.restoreServices')"><el-switch v-model="settingsForm.autoStartServices" /></el-form-item></div>
              <el-divider />
              <h3>{{ t('settings.configTitle') }}</h3><el-text type="info">{{ t('settings.configHelp') }}</el-text>
              <div class="config-actions"><el-button :icon="Import" @click="importConfig">{{ t('settings.importSshConfig') }}</el-button><el-button :icon="FileInput" @click="importAppConfig">{{ t('settings.importAppConfig') }}</el-button><el-button :icon="FileOutput" @click="exportAppConfig">{{ t('settings.exportAppConfig') }}</el-button></div>
            </el-form>
          </el-card>
        </template>
      </el-main>
    </el-container>

    <el-dialog v-model="serverModal" :title="t(serverEditing ? 'serverDialog.editTitle' : 'serverDialog.addTitle')" width="560px" class="server-dialog" align-center destroy-on-close @closed="clearServerSecret">
      <el-form ref="serverFormRef" :model="serverForm" :rules="serverRules" label-position="top">
        <el-form-item :label="t('serverDialog.name')" prop="name"><el-input v-model="serverForm.name" /></el-form-item>
        <el-row :gutter="16"><el-col :span="18"><el-form-item :label="t('serverDialog.host')" prop="host"><el-input v-model="serverForm.host" /></el-form-item></el-col><el-col :span="6"><el-form-item :label="t('serverDialog.port')"><el-input-number v-model="serverForm.port" class="port-input" :min="1" :max="65535" :controls="false" :placeholder="String(DEFAULT_SERVER_PORT)" align="left" /></el-form-item></el-col></el-row>
        <el-row :gutter="16"><el-col :span="18"><el-form-item :label="t('serverDialog.username')" prop="username"><el-input v-model="serverForm.username" /></el-form-item></el-col><el-col :span="6"><el-form-item :label="t('serverDialog.authType')"><el-select v-model="serverForm.authType" @change="serverAuthChanged"><el-option :label="t('serverDialog.key')" value="key" /><el-option :label="t('serverDialog.password')" value="password" /></el-select></el-form-item></el-col></el-row>
        <el-form-item v-if="serverForm.authType === 'key'" :label="t('serverDialog.privateKeyPath')"><el-input v-model="serverForm.privateKeyPath" :placeholder="DEFAULT_PRIVATE_KEY_PATH" /><div class="form-help">{{ t('serverDialog.privateKeyHelp') }}</div></el-form-item>
        <el-form-item class="secret-form-item" :label="t(serverForm.authType === 'password' ? 'serverDialog.password' : 'serverDialog.passphrase')"><el-input v-model="serverSecret" type="password" show-password autocomplete="new-password" /><div class="secret-options"><el-checkbox v-model="serverForm.rememberSecret">{{ t('serverDialog.rememberSecret') }}</el-checkbox><span>{{ t('serverDialog.noPlaintext') }}</span></div></el-form-item>
      </el-form>
      <template #footer><div class="dialog-footer"><el-button v-if="serverEditing" type="danger" plain :icon="Trash2" @click="deleteServer(serverForm)">{{ t('common.delete') }}</el-button><span /><el-button @click="serverModal = false">{{ t('common.cancel') }}</el-button><el-button type="primary" @click="submitServer">{{ t('common.save') }}</el-button></div></template>
    </el-dialog>

    <el-dialog v-model="serviceModal" :title="t(serviceEditing ? 'serviceDialog.editTitle' : 'serviceDialog.addTitle')" width="560px" align-center destroy-on-close>
      <el-form ref="serviceFormRef" :model="serviceForm" :rules="serviceRules" label-position="top">
        <el-row :gutter="16"><el-col :span="18"><el-form-item :label="t('serviceDialog.name')" prop="name"><el-input v-model="serviceForm.name" /></el-form-item></el-col><el-col :span="6"><el-form-item :label="t('serviceDialog.server')" prop="serverId"><el-select v-model="serviceForm.serverId"><el-option v-for="server in servers" :key="server.id" :label="server.name" :value="server.id" /></el-select></el-form-item></el-col></el-row>
        <el-row :gutter="16"><el-col :span="18"><el-form-item :label="t('serviceDialog.remoteHost')"><el-input v-model="serviceForm.remoteHost" :placeholder="DEFAULT_REMOTE_HOST" /></el-form-item></el-col><el-col :span="6"><el-form-item :label="t('serviceDialog.remotePort')"><el-input-number v-model="serviceForm.remotePort" class="port-input" :min="1" :max="65535" :controls="false" :placeholder="String(DEFAULT_REMOTE_PORT)" align="left" /></el-form-item></el-col></el-row>
        <el-form-item :label="t('serviceDialog.domain')"><el-input v-model="serviceDomainPrefix" :placeholder="defaultDomainPrefix()"><template #prepend>http://</template><template #append>.localhost</template></el-input><div class="form-help">{{ t('serviceDialog.domainHelp') }}</div></el-form-item>
        <el-alert type="info" :closable="false" show-icon><template #title><span class="route-summary"><Monitor :size="14" />{{ t('serviceDialog.browser') }} → <code>{{ effectiveServiceDomain() }}</code> → SSH → <code>{{ effectiveRemoteHost() }}:{{ effectiveRemotePort() }}</code></span></template></el-alert>
      </el-form>
      <template #footer><div class="dialog-footer"><el-button v-if="serviceEditing" type="danger" plain :icon="Trash2" @click="deleteService(serviceForm)">{{ t('common.delete') }}</el-button><span /><el-button @click="serviceModal = false">{{ t('common.cancel') }}</el-button><el-button type="primary" @click="submitService">{{ t('common.save') }}</el-button></div></template>
    </el-dialog>
  </el-container>
</template>
