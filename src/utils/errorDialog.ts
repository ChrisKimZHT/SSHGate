import { ElMessageBox } from 'element-plus'
import { i18n } from '../i18n'

let activeDialog: Promise<void> | undefined

function errorMessage(error: unknown) {
  const message = error instanceof Error ? error.message : String(error)
  return message.replace(/^(Error:\s*)+/i, '').trim() || i18n.global.t('error.unknown')
}

export function showError(error: unknown) {
  if (activeDialog) return activeDialog
  activeDialog = ElMessageBox.alert(errorMessage(error), i18n.global.t('error.title'), {
    type: 'error',
    confirmButtonText: i18n.global.t('error.acknowledge'),
    closeOnClickModal: false,
  })
    .then(() => undefined)
    .catch(() => undefined)
    .finally(() => { activeDialog = undefined })
  return activeDialog
}
