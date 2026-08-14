<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { api } from '../api'
import type { TerminalEvent } from '../types'
import { showError } from '../utils/errorDialog'

const props = defineProps<{
  terminalId: string
  serverId: string
  password?: string
}>()
const emit = defineEmits<{ closed: [] }>()

const host = ref<HTMLElement>()
const status = ref('正在打开终端…')
let terminal: Terminal | undefined
let fitAddon: FitAddon | undefined
let observer: ResizeObserver | undefined
let unlistenOutput: UnlistenFn | undefined
let unlistenClosed: UnlistenFn | undefined
let resizeTimer: number | undefined

function decodeBase64(value: string) {
  const raw = atob(value)
  const bytes = new Uint8Array(raw.length)
  for (let i = 0; i < raw.length; i += 1) bytes[i] = raw.charCodeAt(i)
  return bytes
}

async function fitAndResize(send = true) {
  if (!terminal || !fitAddon || !host.value?.clientWidth || !host.value?.clientHeight) return
  fitAddon.fit()
  if (send) await api.terminalResize(props.terminalId, terminal.cols, terminal.rows).catch(() => undefined)
}

onMounted(async () => {
  terminal = new Terminal({
    cursorBlink: true,
    convertEol: false,
    fontFamily: 'JetBrains Mono, Cascadia Code, ui-monospace, SFMono-Regular, Consolas, monospace',
    fontSize: 13,
    lineHeight: 1.25,
    scrollback: 5000,
    theme: {
      background: '#0b1015',
      foreground: '#d7e1e8',
      cursor: '#70e1b2',
      cursorAccent: '#0b1015',
      selectionBackground: '#305b4d99',
      black: '#111820', red: '#ef6b73', green: '#70e1b2', yellow: '#e7c66b',
      blue: '#72a7ff', magenta: '#c493ff', cyan: '#68d5dd', white: '#e7eef2',
      brightBlack: '#60717c', brightRed: '#ff8990', brightGreen: '#91efc8', brightYellow: '#f3d98d',
      brightBlue: '#94bbff', brightMagenta: '#d4aeff', brightCyan: '#8ce8ed', brightWhite: '#ffffff',
    },
  })
  fitAddon = new FitAddon()
  terminal.loadAddon(fitAddon)
  terminal.open(host.value!)
  await fitAndResize(false)

  unlistenOutput = await listen<TerminalEvent>('terminal-output', ({ payload }) => {
    if (payload.terminalId !== props.terminalId || !payload.data) return
    terminal?.write(decodeBase64(payload.data))
  })
  unlistenClosed = await listen<TerminalEvent>('terminal-closed', ({ payload }) => {
    if (payload.terminalId !== props.terminalId) return
    status.value = payload.message || (payload.exitStatus != null ? `远端 Shell 已退出 (${payload.exitStatus})` : '终端已关闭')
    terminal?.writeln(`\r\n\x1b[38;5;244m${status.value}\x1b[0m`)
  })

  terminal.onData((data) => api.terminalInput(props.terminalId, data).catch((error) => {
    status.value = '终端输入失败'
    void showError(error)
  }))

  observer = new ResizeObserver(() => {
    window.clearTimeout(resizeTimer)
    resizeTimer = window.setTimeout(() => void fitAndResize(), 80)
  })
  observer.observe(host.value!)

  try {
    await api.openTerminal(props.serverId, props.terminalId, terminal.cols, terminal.rows, props.password)
    status.value = '已连接'
    terminal.focus()
  } catch (error) {
    status.value = '终端打开失败'
    await showError(error)
  }
})

onBeforeUnmount(() => {
  window.clearTimeout(resizeTimer)
  observer?.disconnect()
  unlistenOutput?.()
  unlistenClosed?.()
  terminal?.dispose()
  void api.closeTerminal(props.terminalId)
})
</script>

<template>
  <div class="terminal-pane">
    <div ref="host" class="terminal-host" />
    <span class="terminal-status">{{ status }}</span>
  </div>
</template>
