import { createApp } from 'vue'
import ElementPlus from 'element-plus'
import zhCn from 'element-plus/es/locale/lang/zh-cn'
import App from './App.vue'
import { i18n } from './i18n'
import 'element-plus/dist/index.css'
import './styles.css'
import '@xterm/xterm/css/xterm.css'

window.addEventListener('keydown', (event) => {
  const isRefreshShortcut = event.key === 'F5'
    || ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'r')

  if (isRefreshShortcut) event.preventDefault()
}, { capture: true })

createApp(App).use(i18n).use(ElementPlus, { locale: zhCn }).mount('#app')
