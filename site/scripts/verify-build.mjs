import { JSDOM } from 'jsdom';
import { readFile, stat, readdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import assert from 'node:assert/strict';
import sharp from 'sharp';
const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../dist',
);
const origin = 'https://liyown.github.io';
const base = '/img/';
async function walk(dir) {
  const files = [];
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) files.push(...(await walk(full)));
    else files.push(full);
  }
  return files;
}
const all = await walk(root);
const htmlFiles = all.filter(
  (x) => x.endsWith('.html') && !x.endsWith('/404.html'),
);
assert.equal(htmlFiles.length, 26, '24 content pages and two 404 pages');
const pages = new Map();
const titles = new Set();
const descriptions = new Set();
for (const file of htmlFiles) {
  const pathname = base + path.relative(root, file).replace(/index\.html$/, '');
  const dom = new JSDOM(await readFile(file, 'utf8'), {
    url: origin + pathname,
  });
  const doc = dom.window.document;
  pages.set(pathname, doc);
  const title = doc.querySelector('title')?.textContent;
  assert.ok(title && !titles.has(title), `Unique title: ${pathname}`);
  titles.add(title);
  const description = doc.querySelector('meta[name=description]')?.content;
  assert.ok(
    description && !descriptions.has(description),
    `Unique description: ${pathname}`,
  );
  descriptions.add(description);
  assert.equal(
    doc.querySelector('link[rel=canonical]')?.href,
    origin + pathname,
  );
  assert.equal(
    doc.documentElement.lang,
    pathname.startsWith('/img/en/') ? 'en' : 'zh-CN',
  );
  assert.equal(doc.querySelectorAll('h1').length, 1, `One h1: ${pathname}`);
  assert.ok(
    doc.querySelector('main')?.textContent.trim().length >
      (pathname.includes('/404/') ? 30 : 90),
    `Static content: ${pathname}`,
  );
  for (const selector of [
    'meta[property="og:title"]',
    'meta[property="og:description"]',
    'meta[property="og:image:alt"]',
    'meta[name="twitter:card"]',
  ])
    assert.ok(doc.querySelector(selector)?.content, `${selector}: ${pathname}`);
  const og = doc.querySelector('meta[property="og:image"]')?.content;
  assert.equal(
    doc.querySelector('meta[property="og:url"]')?.content,
    origin + pathname,
  );
  assert.equal(
    doc.querySelector('meta[property="og:type"]')?.content,
    'website',
  );
  assert.equal(
    doc.querySelector('meta[property="og:image:type"]')?.content,
    'image/png',
  );
  assert.equal(
    doc.querySelector('meta[name="twitter:card"]')?.content,
    'summary_large_image',
  );
  assert.equal(doc.querySelector('meta[name="twitter:image"]')?.content, og);
  assert.equal(
    og,
    origin + base + `og/${pathname.startsWith('/img/en/') ? 'en' : 'zh'}.png`,
  );
  const schemas = JSON.parse(
    doc.querySelector('script[type="application/ld+json"]')?.textContent ||
      '[]',
  );
  assert.ok(schemas.some((x) => x['@type'] === 'WebSite'));
  assert.ok(
    schemas.every((x) => !('offers' in x) && !('aggregateRating' in x)),
    'No invented price or rating',
  );
  if (pathname.includes('/404/'))
    assert.match(
      doc.querySelector('meta[name=robots]')?.content || '',
      /noindex/,
    );
  const ids = [...doc.querySelectorAll('[id]')].map((x) => x.id);
  assert.equal(new Set(ids).size, ids.length, `Unique DOM IDs: ${pathname}`);
}
for (const [pathname, doc] of pages) {
  const local = pathname.replace(/^\/img\/(?:en\/)?/, '');
  for (const [language, prefix] of [
    ['zh-CN', ''],
    ['en', 'en/'],
    ['x-default', ''],
  ]) {
    const target = doc.querySelector(`link[hreflang="${language}"]`)?.href;
    assert.equal(target, origin + base + prefix + local);
    assert.ok(pages.has(new URL(target).pathname));
  }
  const languageLink = doc.querySelector('[data-language]');
  assert.ok(languageLink);
  assert.ok(pages.has(new URL(languageLink.href).pathname));
  for (const node of doc.querySelectorAll(
    'a[href],img[src],link[rel=icon],link[rel=apple-touch-icon],script[src]',
  )) {
    const raw = node.getAttribute('href') || node.getAttribute('src');
    if (!raw || raw.startsWith('mailto:')) continue;
    const url = new URL(raw, origin + pathname);
    if (url.origin !== origin) continue;
    assert.ok(
      url.pathname.startsWith(base),
      `Base path: ${raw} on ${pathname}`,
    );
    if (pages.has(url.pathname)) {
      if (url.hash)
        assert.ok(
          pages
            .get(url.pathname)
            .getElementById(decodeURIComponent(url.hash.slice(1))),
          `Anchor ${raw} on ${pathname}`,
        );
    } else {
      const file = path.join(
        root,
        decodeURIComponent(url.pathname.slice(base.length)),
      );
      assert.ok((await stat(file)).isFile(), `Asset ${raw} on ${pathname}`);
    }
  }
  for (const img of doc.querySelectorAll('img[src]')) {
    assert.ok(img.alt, `Image alt ${pathname}`);
    assert.ok(
      Number(img.width) > 0 && Number(img.height) > 0,
      `Image dimensions ${pathname}`,
    );
  }
}
const sitemap = new JSDOM(
  await readFile(path.join(root, 'sitemap.xml'), 'utf8'),
  { contentType: 'text/xml' },
).window.document;
assert.equal(sitemap.querySelectorAll('url').length, 24);
for (const loc of sitemap.querySelectorAll('loc')) {
  assert.ok(pages.has(new URL(loc.textContent).pathname));
  assert.ok(!loc.textContent.includes('404'));
}
for (const locale of ['zh', 'en']) {
  const meta = await sharp(path.join(root, `og/${locale}.png`)).metadata();
  assert.equal(meta.width, 1200);
  assert.equal(meta.height, 630);
}
assert.ok(
  (await readFile(path.join(root, '404.html'), 'utf8')).includes('noindex'),
);
console.log(
  `Verified ${pages.size} bilingual pages: titles, descriptions, language pairs, canonical URLs, static content, local links, anchors, assets, sitemap, schema, OG dimensions, and Pages 404 fallback.`,
);
