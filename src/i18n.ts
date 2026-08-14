import { createI18n } from 'vue-i18n'
import zhCN from './locales/zh-CN'

export type MessageSchema = typeof zhCN
export type SupportedLocale = 'zh-CN'

export const i18n = createI18n<[MessageSchema], SupportedLocale>({
  legacy: false,
  locale: 'zh-CN',
  fallbackLocale: 'zh-CN',
  messages: {
    'zh-CN': zhCN,
  },
})
