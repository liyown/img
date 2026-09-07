import { defineConfig } from 'astro/config';
export default defineConfig({
  site: 'https://liyown.github.io',
  base: '/img',
  trailingSlash: 'always',
  output: 'static',
  devToolbar: { enabled: false },
  markdown: { shikiConfig: { theme: 'github-light' } },
});
