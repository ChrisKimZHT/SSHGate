import { ElMessageBox } from 'element-plus'

let activeDialog: Promise<void> | undefined

function errorMessage(error: unknown) {
  const message = error instanceof Error ? error.message : String(error)
  return message.replace(/^(Error:\s*)+/i, '').trim() || '发生未知错误'
}

export function showError(error: unknown) {
  if (activeDialog) return activeDialog
  activeDialog = ElMessageBox.alert(errorMessage(error), '操作失败', {
    type: 'error',
    confirmButtonText: '知道了',
    closeOnClickModal: false,
  })
    .then(() => undefined)
    .catch(() => undefined)
    .finally(() => { activeDialog = undefined })
  return activeDialog
}
