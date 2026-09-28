import { createI18n } from 'vue-i18n'
import zhCN from './locales/zh-CN'
import enUS from './locales/en-US'

export type MessageSchema = typeof zhCN
export type SupportedLocale = 'zh-CN' | 'en-US'

export const i18n = createI18n<[MessageSchema], SupportedLocale>({
  legacy: false,
  locale: 'zh-CN',
  fallbackLocale: 'zh-CN',
  messages: {
    'zh-CN': zhCN,
    'en-US': enUS,
  },
})
