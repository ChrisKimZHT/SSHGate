import { createApp } from 'vue'
import ElementPlus from 'element-plus'
import zhCn from 'element-plus/es/locale/lang/zh-cn'
import App from './App.vue'
import 'element-plus/dist/index.css'
import './styles.css'
import '@xterm/xterm/css/xterm.css'

createApp(App).use(ElementPlus, { locale: zhCn }).mount('#app')
