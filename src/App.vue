<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { openUrl } from '@tauri-apps/plugin-opener'
import { ElMessage, ElMessageBox, type FormInstance, type FormRules, type TagProps } from 'element-plus'
import {
  CirclePlus, Copy, ExternalLink, Import, Monitor, Moon, Network, Plus, RefreshCw,
  Server, Settings as Setting, Sun as Sunny, TerminalSquare, Trash2,
} from 'lucide-vue-next'
import { api } from './api'
import TerminalPane from './components/TerminalPane.vue'
import type { RuntimeSnapshot, Settings, SshServer, WebService } from './types'

type Page = 'servers' | 'services' | 'terminal' | 'settings'
interface TerminalTab { id: string; serverId: string; title: string; password?: string }

const page = ref<Page>('servers')
const snapshot = ref<RuntimeSnapshot>()
const loading = ref(true)
const globalError = ref('')
const serverModal = ref(false)
const serviceModal = ref(false)
const serverEditing = ref(false)
const serviceEditing = ref(false)
const serverFormRef = ref<FormInstance>()
const serviceFormRef = ref<FormInstance>()
const passwordByServer = reactive<Record<string, string>>({})
const terminalTabs = ref<TerminalTab[]>([])
const activeTerminalId = ref('')
const darkMode = ref(localStorage.getItem('sshgate-theme') === 'dark')
let unlistenState: UnlistenFn | undefined

const blankServer = (): SshServer => ({
  id: crypto.randomUUID(), name: '', host: '', port: 22, username: '', authType: 'key', privateKeyPath: '~/.ssh/id_ed25519',
})
const blankService = (serverId = ''): WebService => ({
  id: crypto.randomUUID(), serverId, name: '', remoteHost: '127.0.0.1', remotePort: 3000, domain: '', desiredRunning: false,
})
const serverForm = reactive<SshServer>(blankServer())
const serviceForm = reactive<WebService>(blankService())
const settingsForm = reactive<Settings>({ listenAddress: '127.0.0.1', listenPort: 80, reconnectDelaySeconds: 3, autoStartServices: true })

const serverRules: FormRules = {
  name: [{ required: true, message: '请输入服务器名称', trigger: 'blur' }],
  host: [{ required: true, message: '请输入主机地址', trigger: 'blur' }],
  username: [{ required: true, message: '请输入用户名', trigger: 'blur' }],
  privateKeyPath: [{ required: true, message: '请输入私钥路径', trigger: 'blur' }],
}
const serviceRules: FormRules = {
  name: [{ required: true, message: '请输入服务名称', trigger: 'blur' }],
  serverId: [{ required: true, message: '请选择 SSH 服务器', trigger: 'change' }],
  remoteHost: [{ required: true, message: '请输入远端主机', trigger: 'blur' }],
  domain: [
    { required: true, message: '请输入域名', trigger: 'blur' },
    { pattern: /^[a-zA-Z0-9.-]+\.localhost$/, message: '必须是有效的 .localhost 域名', trigger: 'blur' },
  ],
}

const servers = computed(() => snapshot.value?.config.servers ?? [])
const services = computed(() => snapshot.value?.config.services ?? [])
const runningCount = computed(() => Object.values(snapshot.value?.serviceStates ?? {}).filter((state) => state.status === 'running').length)
const proxyHealthy = computed(() => !snapshot.value?.proxyError)

function applyTheme() {
  document.documentElement.classList.toggle('dark', darkMode.value)
  localStorage.setItem('sshgate-theme', darkMode.value ? 'dark' : 'light')
}
function slug(value: string) { return value.trim().toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '') || 'service' }
function serverById(id: string) { return servers.value.find((server) => server.id === id) }
function servicesFor(id: string) { return services.value.filter((service) => service.serverId === id) }
function serverState(id: string) { return snapshot.value?.serverStates[id] ?? { status: 'stopped' as const } }
function serviceState(id: string) { return snapshot.value?.serviceStates[id] ?? { status: 'stopped' as const } }
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
    Object.assign(settingsForm, snapshot.value.config.settings)
    globalError.value = ''
  } catch (error) { globalError.value = String(error) }
  finally { loading.value = false }
}
function showAddServer() { Object.assign(serverForm, blankServer()); serverEditing.value = false; serverModal.value = true; nextTick(() => serverFormRef.value?.clearValidate()) }
function showEditServer(server: SshServer) { Object.assign(serverForm, server); serverEditing.value = true; serverModal.value = true; nextTick(() => serverFormRef.value?.clearValidate()) }
function showAddService(serverId = servers.value[0]?.id ?? '') { Object.assign(serviceForm, blankService(serverId)); serviceEditing.value = false; serviceModal.value = true; nextTick(() => serviceFormRef.value?.clearValidate()) }
function showEditService(service: WebService) { Object.assign(serviceForm, service); serviceEditing.value = true; serviceModal.value = true; nextTick(() => serviceFormRef.value?.clearValidate()) }
function suggestDomain(force = false) {
  if (serviceEditing.value && !force) return
  const server = serverById(serviceForm.serverId)
  if (server && serviceForm.name) serviceForm.domain = `${slug(serviceForm.name)}.${slug(server.name)}.localhost`
}

async function run(action: () => Promise<RuntimeSnapshot | void>, success?: string) {
  try {
    const result = await action()
    if (result) snapshot.value = result as RuntimeSnapshot
    globalError.value = ''
    if (success) ElMessage.success(success)
    return true
  } catch (error) {
    globalError.value = String(error)
    ElMessage.error(globalError.value)
    return false
  }
}
async function submitServer() {
  if (!await serverFormRef.value?.validate().catch(() => false)) return
  if (await run(() => api.saveServer({ ...serverForm }), '服务器已保存')) serverModal.value = false
}
async function submitService() {
  serviceForm.domain = serviceForm.domain.toLowerCase().replace(/^https?:\/\//, '').replace(/\/$/, '')
  if (!await serviceFormRef.value?.validate().catch(() => false)) return
  if (await run(() => api.saveService({ ...serviceForm }), '服务已保存')) serviceModal.value = false
}
async function deleteServer(server: SshServer) {
  try { await ElMessageBox.confirm(`删除“${server.name}”及其全部服务？`, '删除服务器', { type: 'warning', confirmButtonText: '删除', cancelButtonText: '取消' }) }
  catch { return }
  if (await run(() => api.removeServer(server.id), '服务器已删除')) serverModal.value = false
}
async function deleteService(service: WebService) {
  try { await ElMessageBox.confirm(`确定删除服务“${service.name}”？`, '删除服务', { type: 'warning', confirmButtonText: '删除', cancelButtonText: '取消' }) }
  catch { return }
  await run(() => api.removeService(service.id), '服务已删除')
}
async function toggleServer(server: SshServer) {
  const connected = serverState(server.id).status === 'connected'
  await run(() => connected ? api.disconnectServer(server.id) : api.connectServer(server.id, passwordByServer[server.id]), connected ? '服务器已断开' : 'SSH 连接成功')
}
async function toggleService(service: WebService) {
  const running = ['running', 'starting', 'reconnecting'].includes(serviceState(service.id).status)
  await run(() => running ? api.stopService(service.id) : api.startService(service.id, passwordByServer[service.serverId]), running ? '服务已停止' : '服务已启动')
}
function serviceUrl(service: WebService) {
  const port = snapshot.value?.config.settings.listenPort ?? 80
  return `http://${service.domain}${port === 80 ? '' : `:${port}`}`
}
async function openService(service: WebService) { await openUrl(serviceUrl(service)) }
async function copyDomain(service: WebService) { await navigator.clipboard.writeText(serviceUrl(service)); ElMessage.success('访问地址已复制') }
function openTerminal(server: SshServer) {
  const id = crypto.randomUUID()
  terminalTabs.value.push({ id, serverId: server.id, title: server.name, password: passwordByServer[server.id] })
  activeTerminalId.value = id
  page.value = 'terminal'
}
function closeTerminal(id: string) {
  const index = terminalTabs.value.findIndex((tab) => tab.id === id)
  terminalTabs.value = terminalTabs.value.filter((tab) => tab.id !== id)
  if (activeTerminalId.value === id) activeTerminalId.value = terminalTabs.value[Math.max(0, index - 1)]?.id ?? ''
}
function newTerminal() { if (servers.value[0]) openTerminal(servers.value[0]) }
async function importConfig() {
  try {
    const imported = await api.importSshConfig(); await refresh()
    imported.length ? ElMessage.success(`已导入 ${imported.length} 台服务器`) : ElMessage.info('没有发现可导入的 Host')
  } catch (error) { ElMessage.error(String(error)) }
}
async function saveSettings() { await run(() => api.saveSettings({ ...settingsForm }), '设置已保存，代理已重启') }

onMounted(async () => {
  applyTheme()
  unlistenState = await listen<RuntimeSnapshot>('state-changed', ({ payload }) => { snapshot.value = payload })
  await refresh()
})
onBeforeUnmount(() => unlistenState?.())
</script>

<template>
  <el-container class="app-frame">
    <el-aside width="232px" class="app-sidebar">
      <div class="brand"><span class="brand-icon"><TerminalSquare :size="20" /></span><div><strong>SSHGate</strong><small>Secure local gateway</small></div></div>
      <el-menu :default-active="page" class="nav-menu" @select="page = $event as Page">
        <el-menu-item index="servers"><Server :size="18" /><span>服务器</span></el-menu-item>
        <el-menu-item index="services"><Network :size="18" /><span>Web 服务</span><el-badge v-if="runningCount" :value="runningCount" type="success" /></el-menu-item>
        <el-menu-item index="terminal"><TerminalSquare :size="18" /><span>终端</span><el-badge v-if="terminalTabs.length" :value="terminalTabs.length" /></el-menu-item>
        <el-menu-item index="settings"><Setting :size="18" /><span>设置</span></el-menu-item>
      </el-menu>
      <div class="sidebar-footer">
        <el-card shadow="never" class="sidebar-control-card">
          <div class="proxy-line"><el-badge is-dot :type="proxyHealthy ? 'success' : 'danger'" /><div><b>本地代理</b><small>{{ snapshot?.proxyError || `${settingsForm.listenAddress}:${settingsForm.listenPort}` }}</small></div></div>
          <el-divider />
          <div class="theme-control">
            <div class="theme-label"><component :is="darkMode ? Moon : Sunny" :size="16" /><div><b>外观</b><small>{{ darkMode ? '深色模式' : '浅色模式' }}</small></div></div>
            <el-switch v-model="darkMode" :active-action-icon="Moon" :inactive-action-icon="Sunny" aria-label="切换深色模式" @change="applyTheme" />
          </div>
        </el-card>
      </div>
    </el-aside>

    <el-container class="content-shell">
      <el-header class="topbar">
        <el-breadcrumb separator="/"><el-breadcrumb-item>SSHGate</el-breadcrumb-item><el-breadcrumb-item>{{ { servers: '服务器', services: 'Web 服务', terminal: '终端', settings: '设置' }[page] }}</el-breadcrumb-item></el-breadcrumb>
        <el-button circle text :icon="RefreshCw" title="刷新状态" @click="refresh" />
      </el-header>
      <el-main :class="['page-content', { 'terminal-content': page === 'terminal' }]">
        <el-alert v-if="globalError" class="global-alert" :title="globalError" type="error" show-icon closable @close="globalError = ''" />

        <template v-if="page === 'servers'">
          <div class="page-heading"><div><h1>服务器</h1><p>管理 SSH 连接、远端服务和交互式终端。</p></div><el-space><el-button :icon="Import" @click="importConfig">导入 SSH Config</el-button><el-button type="primary" :icon="Plus" @click="showAddServer">添加服务器</el-button></el-space></div>
          <el-skeleton :loading="loading" animated :rows="6">
            <div v-if="servers.length" class="server-grid">
              <el-card v-for="server in servers" :key="server.id" shadow="hover" class="server-card">
                <template #header><div class="server-header"><el-avatar shape="square" :size="44"><Server :size="22" /></el-avatar><div class="server-title"><h3>{{ server.name }}</h3><el-text type="info" truncated><code>{{ server.username }}@{{ server.host }}:{{ server.port }}</code></el-text></div><el-tag :type="stateType(serverState(server.id).status)" effect="light" round>{{ stateLabel(serverState(server.id).status) }}</el-tag><el-button text circle @click="showEditServer(server)"><Setting :size="16" /></el-button></div></template>
                <el-alert v-if="serverState(server.id).error" :title="serverState(server.id).error || ''" type="error" :closable="false" show-icon />
                <el-input v-if="server.authType === 'password' && serverState(server.id).status !== 'connected'" v-model="passwordByServer[server.id]" class="password-input" type="password" show-password placeholder="本次连接密码（不会保存）" />
                <el-space wrap class="server-actions"><el-button :icon="TerminalSquare" @click="openTerminal(server)">打开终端</el-button><el-button :icon="CirclePlus" @click="showAddService(server.id)">添加服务</el-button><el-button :type="serverState(server.id).status === 'connected' ? 'default' : 'primary'" :loading="['connecting', 'reconnecting'].includes(serverState(server.id).status)" @click="toggleServer(server)">{{ serverState(server.id).status === 'connected' ? '断开连接' : '连接' }}</el-button></el-space>
                <el-divider content-position="left">服务 · {{ servicesFor(server.id).length }}</el-divider>
                <el-empty v-if="!servicesFor(server.id).length" :image-size="46" description="尚未添加服务" />
                <el-table v-else :data="servicesFor(server.id)" size="small" :show-header="false" class="embedded-table">
                  <el-table-column min-width="170"><template #default="{ row }"><div class="service-name"><b>{{ row.name }}</b><el-link type="primary" :underline="false" @click="copyDomain(row)">{{ row.domain }}</el-link></div></template></el-table-column>
                  <el-table-column width="125"><template #default="{ row }"><el-text type="info"><code>{{ row.remoteHost }}:{{ row.remotePort }}</code></el-text></template></el-table-column>
                  <el-table-column width="94" align="right"><template #default="{ row }"><el-space :size="4"><el-button circle text :icon="ExternalLink" :disabled="serviceState(row.id).status !== 'running'" @click="openService(row)" /><el-switch :model-value="serviceState(row.id).status === 'running'" :loading="['starting', 'reconnecting'].includes(serviceState(row.id).status)" @change="toggleService(row)" /></el-space></template></el-table-column>
                </el-table>
              </el-card>
            </div>
            <el-empty v-else description="还没有 SSH 服务器"><el-button type="primary" :icon="Plus" @click="showAddServer">添加第一台服务器</el-button></el-empty>
          </el-skeleton>
        </template>

        <template v-else-if="page === 'services'">
          <div class="page-heading"><div><h1>Web 服务</h1><p>{{ runningCount }} 个服务正在通过 SSH 隧道提供访问。</p></div><el-button type="primary" :icon="Plus" :disabled="!servers.length" @click="showAddService()">添加服务</el-button></div>
          <el-card shadow="never" class="table-card">
            <el-table v-if="services.length" :data="services" stripe>
              <el-table-column label="服务" min-width="220"><template #default="{ row }"><div class="service-name"><b>{{ row.name }}</b><el-link type="primary" :underline="false" @click="copyDomain(row)">{{ serviceUrl(row) }} <Copy :size="12" /></el-link></div></template></el-table-column>
              <el-table-column label="服务器" width="150"><template #default="{ row }">{{ serverById(row.serverId)?.name }}</template></el-table-column>
              <el-table-column label="远端地址" width="170"><template #default="{ row }"><code>{{ row.remoteHost }}:{{ row.remotePort }}</code></template></el-table-column>
              <el-table-column label="状态" width="120"><template #default="{ row }"><el-tag :type="stateType(serviceState(row.id).status)" effect="light" round>{{ stateLabel(serviceState(row.id).status) }}</el-tag></template></el-table-column>
              <el-table-column label="操作" width="230" align="right"><template #default="{ row }"><el-button text type="primary" :icon="ExternalLink" :disabled="serviceState(row.id).status !== 'running'" @click="openService(row)">打开</el-button><el-button text @click="toggleService(row)">{{ serviceState(row.id).status === 'running' ? '停止' : '启动' }}</el-button><el-button text @click="showEditService(row)">编辑</el-button><el-button text type="danger" :icon="Trash2" @click="deleteService(row)" /></template></el-table-column>
            </el-table>
            <el-empty v-else description="还没有 Web 服务"><el-button type="primary" :icon="Plus" :disabled="!servers.length" @click="showAddService()">添加服务</el-button></el-empty>
          </el-card>
        </template>

        <template v-else-if="page === 'terminal'">
          <div class="terminal-workspace">
            <el-tabs v-if="terminalTabs.length" v-model="activeTerminalId" type="card" closable class="terminal-tabs" @tab-remove="closeTerminal(String($event))">
              <el-tab-pane v-for="tab in terminalTabs" :key="tab.id" :name="tab.id" :label="tab.title"><TerminalPane :terminal-id="tab.id" :server-id="tab.serverId" :password="tab.password" @closed="closeTerminal(tab.id)" /></el-tab-pane>
            </el-tabs>
            <div class="terminal-toolbar"><el-button size="small" :icon="Plus" :disabled="!servers.length" @click="newTerminal">新建终端</el-button></div>
            <el-empty v-if="!terminalTabs.length" description="选择一台服务器打开内嵌 SSH 终端"><el-space wrap><el-button v-for="server in servers" :key="server.id" :icon="TerminalSquare" @click="openTerminal(server)">{{ server.name }}</el-button></el-space></el-empty>
          </div>
        </template>

        <template v-else>
          <div class="page-heading"><div><h1>设置</h1><p>调整本地代理入口、自动恢复和显示主题。</p></div></div>
          <el-card shadow="never" class="settings-card">
            <el-form :model="settingsForm" label-position="top">
              <h3>HTTP 反向代理</h3><el-text type="info">端口 80 可直接使用 http://*.localhost；端口被占用时可在这里修改。</el-text>
              <el-row :gutter="20" class="settings-row"><el-col :span="14"><el-form-item label="监听地址"><el-input v-model="settingsForm.listenAddress" /></el-form-item></el-col><el-col :span="10"><el-form-item label="监听端口"><el-input-number v-model="settingsForm.listenPort" :min="1" :max="65535" controls-position="right" /></el-form-item></el-col></el-row>
              <el-divider />
              <h3>连接恢复</h3><el-row :gutter="20" class="settings-row"><el-col :span="12"><el-form-item label="重连间隔（秒）"><el-input-number v-model="settingsForm.reconnectDelaySeconds" :min="1" :max="300" controls-position="right" /></el-form-item></el-col><el-col :span="12"><el-form-item label="启动时恢复服务"><el-switch v-model="settingsForm.autoStartServices" inline-prompt active-text="开启" inactive-text="关闭" /></el-form-item></el-col></el-row>
              <el-divider />
              <div class="theme-setting"><div><h3>界面主题</h3><el-text type="info">跟随你的偏好在浅色和深色主题之间切换。</el-text></div><el-segmented v-model="darkMode" :options="[{ label: '浅色', value: false }, { label: '深色', value: true }]" @change="applyTheme" /></div>
              <div class="form-footer"><el-button type="primary" @click="saveSettings">保存设置</el-button></div>
            </el-form>
          </el-card>
        </template>
      </el-main>
    </el-container>

    <el-dialog v-model="serverModal" :title="serverEditing ? '编辑服务器' : '添加服务器'" width="560px" destroy-on-close>
      <el-form ref="serverFormRef" :model="serverForm" :rules="serverRules" label-position="top">
        <el-form-item label="名称" prop="name"><el-input v-model="serverForm.name" placeholder="GPU Server" /></el-form-item>
        <el-row :gutter="16"><el-col :span="18"><el-form-item label="主机" prop="host"><el-input v-model="serverForm.host" placeholder="192.168.1.100" /></el-form-item></el-col><el-col :span="6"><el-form-item label="端口"><el-input-number v-model="serverForm.port" :min="1" :max="65535" :controls="false" /></el-form-item></el-col></el-row>
        <el-row :gutter="16"><el-col :span="14"><el-form-item label="用户名" prop="username"><el-input v-model="serverForm.username" placeholder="root" /></el-form-item></el-col><el-col :span="10"><el-form-item label="认证方式"><el-select v-model="serverForm.authType"><el-option label="SSH 私钥" value="key" /><el-option label="密码（不保存）" value="password" /></el-select></el-form-item></el-col></el-row>
        <el-form-item v-if="serverForm.authType === 'key'" label="私钥路径" prop="privateKeyPath"><el-input v-model="serverForm.privateKeyPath" placeholder="~/.ssh/id_ed25519" /><div class="form-help">仅保存路径，不复制私钥内容。</div></el-form-item>
        <el-descriptions v-if="serverForm.hostKeyFingerprint" :column="1" border size="small"><el-descriptions-item label="已信任主机指纹"><code>{{ serverForm.hostKeyFingerprint }}</code></el-descriptions-item></el-descriptions>
      </el-form>
      <template #footer><div class="dialog-footer"><el-button v-if="serverEditing" type="danger" plain :icon="Trash2" @click="deleteServer(serverForm)">删除</el-button><span /><el-button @click="serverModal = false">取消</el-button><el-button type="primary" @click="submitServer">保存</el-button></div></template>
    </el-dialog>

    <el-dialog v-model="serviceModal" :title="serviceEditing ? '编辑服务' : '添加 Web 服务'" width="560px" destroy-on-close>
      <el-form ref="serviceFormRef" :model="serviceForm" :rules="serviceRules" label-position="top">
        <el-row :gutter="16"><el-col :span="14"><el-form-item label="名称" prop="name"><el-input v-model="serviceForm.name" placeholder="Jupyter" @input="suggestDomain()" /></el-form-item></el-col><el-col :span="10"><el-form-item label="SSH 服务器" prop="serverId"><el-select v-model="serviceForm.serverId" @change="suggestDomain(true)"><el-option v-for="server in servers" :key="server.id" :label="server.name" :value="server.id" /></el-select></el-form-item></el-col></el-row>
        <el-row :gutter="16"><el-col :span="16"><el-form-item label="远端主机" prop="remoteHost"><el-input v-model="serviceForm.remoteHost" /></el-form-item></el-col><el-col :span="8"><el-form-item label="远端端口"><el-input-number v-model="serviceForm.remotePort" :min="1" :max="65535" :controls="false" /></el-form-item></el-col></el-row>
        <el-form-item label=".localhost 域名" prop="domain"><el-input v-model="serviceForm.domain" placeholder="jupyter.gpu.localhost"><template #prepend>http://</template></el-input><div class="form-help">支持多级域名，无需修改 hosts 或配置 DNS。</div></el-form-item>
        <el-alert type="info" :closable="false" show-icon><template #title><span class="route-summary"><Monitor :size="14" />浏览器 → <code>{{ serviceForm.domain || 'service.server.localhost' }}</code> → SSH → <code>{{ serviceForm.remoteHost }}:{{ serviceForm.remotePort }}</code></span></template></el-alert>
      </el-form>
      <template #footer><el-button @click="serviceModal = false">取消</el-button><el-button type="primary" @click="submitService">保存</el-button></template>
    </el-dialog>
  </el-container>
</template>
