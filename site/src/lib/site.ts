export type Locale = 'zh' | 'en';
export const site = {
  origin: 'https://liyown.github.io',
  base: '/img/',
  repository: 'https://github.com/liyown/img',
  productCommit: '3d8e8b9',
  verified: '2026-09-08',
  gaId: import.meta.env.PUBLIC_GA_MEASUREMENT_ID?.trim() ?? '',
  verification: import.meta.env.PUBLIC_GOOGLE_SITE_VERIFICATION?.trim() ?? '',
};
if (site.gaId && !/^G-[A-Z0-9]+$/.test(site.gaId))
  throw new Error('Invalid PUBLIC_GA_MEASUREMENT_ID');
export const locales: Locale[] = ['zh', 'en'];
export function href(locale: Locale, path = '') {
  return `${site.base}${locale === 'en' ? 'en/' : ''}${path ? path.replace(/^\/+|\/+$/g, '') + '/' : ''}`;
}
export function absolute(locale: Locale, path = '') {
  return `${site.origin}${href(locale, path)}`;
}
export const asset = (path: string) => site.base + path.replace(/^\//, '');
export const t = <T>(locale: Locale, zh: T, en: T): T =>
  locale === 'zh' ? zh : en;
export const pages = [
  ['', '首页', 'Home'],
  ['features', '功能', 'Features'],
  ['compare', '竞品对比', 'Compare'],
  ['install', '安装', 'Install'],
  ['docs', '文档', 'Documentation'],
  ['docs/quick-start', '快速开始', 'Quick start'],
  ['docs/storage', '存储配置', 'Storage'],
  ['docs/desktop', '桌面使用', 'Desktop guide'],
  ['docs/cli', 'CLI 参考', 'CLI reference'],
  ['docs/integrations', '编辑器与 Agent', 'Editors & agents'],
  ['docs/troubleshooting', '故障排查', 'Troubleshooting'],
  ['docs/0.4', '0.4 新功能', 'What’s new in 0.4'],
  ['docs/workflows', '迁移与资料管理', 'Migration & data management'],
  ['privacy', '隐私说明', 'Privacy'],
] as const;
export const pageName = (locale: Locale, path: string) => {
  const page = pages.find((p) => p[0] === path);
  return page ? page[locale === 'zh' ? 1 : 2] : '404';
};
