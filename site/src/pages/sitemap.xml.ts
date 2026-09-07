import { pages, locales, absolute } from '../lib/site';
export function GET() {
  const urls = locales.flatMap((locale) =>
    pages.map(
      ([path]) =>
        `<url><loc>${absolute(locale, path)}</loc><xhtml:link rel="alternate" hreflang="zh-CN" href="${absolute('zh', path)}"/><xhtml:link rel="alternate" hreflang="en" href="${absolute('en', path)}"/><xhtml:link rel="alternate" hreflang="x-default" href="${absolute('zh', path)}"/></url>`,
    ),
  );
  return new Response(
    `<?xml version="1.0" encoding="UTF-8"?><urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9" xmlns:xhtml="http://www.w3.org/1999/xhtml">${urls.join('')}</urlset>`,
    { headers: { 'Content-Type': 'application/xml' } },
  );
}
