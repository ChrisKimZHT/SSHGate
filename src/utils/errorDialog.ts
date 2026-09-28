import { ElMessageBox } from 'element-plus'
import { ref } from 'vue'
import { i18n } from '../i18n'
import type { HostKeyChange } from '../types'

export const hostKeyChanges = ref<HostKeyChange[]>([])
const seenHostKeyChanges = new Set<string>()

function queueHostKeyChange(message: string) {
  const prefix = 'SSH 主机指纹已改变:'
  if (!message.startsWith(prefix)) return false
  try {
    const change = JSON.parse(message.slice(prefix.length)) as HostKeyChange
    if (typeof change.requestId !== 'string' || typeof change.serverName !== 'string'
      || typeof change.host !== 'string' || typeof change.port !== 'number'
      || typeof change.savedFingerprint !== 'string' || typeof change.fingerprint !== 'string') return false
    // The same failure arrives through both the background event and the command result.
    if (!seenHostKeyChanges.has(change.requestId)) {
      seenHostKeyChanges.add(change.requestId)
      if (seenHostKeyChanges.size > 100) seenHostKeyChanges.delete(seenHostKeyChanges.values().next().value!)
      hostKeyChanges.value.push(change)
    }
    return true
  } catch { return false }
}

let activeDialog: Promise<void> | undefined

function errorMessage(error: unknown) {
  const message = error instanceof Error ? error.message : String(error)
  return message.replace(/^(Error:\s*)+/i, '').trim() || i18n.global.t('error.unknown')
}

export function showError(error: unknown) {
  const message = errorMessage(error)
  // Dismissing the trust prompt already expresses the user's intent to cancel.
  if (message === '未信任 SSH 主机指纹，已取消连接') return Promise.resolve()
  if (queueHostKeyChange(message)) return Promise.resolve()
  if (activeDialog) return activeDialog
  activeDialog = ElMessageBox.alert(message, i18n.global.t('error.title'), {
    type: 'error',
    confirmButtonText: i18n.global.t('error.acknowledge'),
    closeOnClickModal: false,
  })
    .then(() => undefined)
    .catch(() => undefined)
    .finally(() => { activeDialog = undefined })
  return activeDialog
}
