import { createApp } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import ElementPlus from 'element-plus'
import zhCn from 'element-plus/es/locale/lang/zh-cn'
import App from './App.vue'
import { i18n } from './i18n'
import 'element-plus/dist/index.css'
import 'element-plus/theme-chalk/dark/css-vars.css'
import './styles.css'
import '@xterm/xterm/css/xterm.css'

const initialTheme = localStorage.getItem('sshgate-theme') === 'dark' ? 'dark' : 'light'
document.documentElement.classList.toggle('dark', initialTheme === 'dark')
void getCurrentWindow().setTheme(initialTheme).catch(() => {})

window.addEventListener('keydown', (event) => {
  const isRefreshShortcut = event.key === 'F5'
    || ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'r')

  if (isRefreshShortcut) event.preventDefault()
}, { capture: true })

createApp(App).use(i18n).use(ElementPlus, { locale: zhCn }).mount('#app')
