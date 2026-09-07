import sharp from 'sharp';
import { mkdir, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const svgIcon = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect x="1" y="1" width="62" height="62" rx="17" fill="#f8f7f3"/><g fill="none" stroke="#252923" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round" transform="rotate(-6 32 32)"><rect x="15" y="15" width="34" height="34" rx="6"/><path d="m16 39 10-11 9 9 5-5 9 10"/><circle cx="39" cy="25" r="2" fill="#a96412" stroke="none"/></g></svg>`;
await mkdir(path.join(root, 'public/og'), { recursive: true });
await mkdir(path.join(root, 'public/screenshots'), { recursive: true });
await writeFile(path.join(root, 'public/favicon.svg'), svgIcon);
await sharp(Buffer.from(svgIcon))
  .resize(180, 180)
  .png()
  .toFile(path.join(root, 'public/apple-touch-icon.png'));
for (const name of ['gallery', 'queue', 'settings']) {
  const source = path.join(root, `src/assets/screenshots/${name}.png`);
  await sharp(source)
    .webp({ lossless: true })
    .toFile(path.join(root, `public/screenshots/${name}.webp`));
  for (const width of [640, 896, 960])
    await sharp(source)
      .resize(width)
      .webp({ lossless: true })
      .toFile(path.join(root, `public/screenshots/${name}-${width}.webp`));
}
for (const locale of ['zh', 'en']) {
  const zh = locale === 'zh';
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="630" viewBox="0 0 1200 630"><rect width="1200" height="630" fill="#f8f7f3"/><path d="M60 96h1080M60 563h1080" stroke="#d6dbca"/><text x="63" y="65" font-family="Arial,sans-serif" font-size="37" font-weight="700" fill="#252923">img<tspan fill="#a96412">.</tspan></text><text x="1140" y="61" text-anchor="end" font-family="monospace" font-size="14" fill="#656960">NATIVE IMAGE UPLOADER</text><text x="65" y="243" font-family="PingFang SC,Arial,sans-serif" font-size="${zh ? 88 : 84}" font-weight="500" fill="#252923">${zh ? '从图片，' : 'From image,'}</text><text x="65" y="345" font-family="PingFang SC,Arial,sans-serif" font-size="${zh ? 88 : 84}" font-weight="500" fill="#536344">${zh ? '到链接。' : 'to link.'}</text><text x="69" y="417" font-family="PingFang SC,Arial,sans-serif" font-size="22" fill="#656960">${zh ? '原生桌面、独立 CLI 与 Agent Skill。支持自有存储。' : 'Native desktop, standalone CLI, and Agent Skill.'}</text><rect x="66" y="460" width="170" height="44" rx="5" fill="#252923"/><text x="151" y="489" text-anchor="middle" font-family="Arial,sans-serif" font-size="15" fill="#fffefa">Native desktop</text><rect x="248" y="460" width="135" height="44" rx="5" fill="none" stroke="#b9c1ad"/><text x="315" y="489" text-anchor="middle" font-family="Arial,sans-serif" font-size="15" fill="#252923">Rust CLI</text><text x="65" y="601" font-family="monospace" font-size="14" fill="#656960">liyown.github.io/img</text><text x="1140" y="601" text-anchor="end" font-family="monospace" font-size="12" fill="#656960">macOS / Windows / Linux</text><g transform="translate(900 250) rotate(-8)"><rect x="-95" y="-100" width="220" height="220" rx="50" fill="#edeada" stroke="#d0d5c3"/><g fill="none" stroke="#536344" stroke-width="6" stroke-linecap="round" stroke-linejoin="round"><rect x="-40" y="-44" width="112" height="112" rx="17"/><path d="m-38 35 35-39 31 31 18-18 25 28"/><circle cx="40" cy="-12" r="8" fill="#c17722" stroke="none"/></g></g></svg>`;
  await writeFile(path.join(root, `public/og/${locale}.svg`), svg);
  await sharp(Buffer.from(svg))
    .png()
    .toFile(path.join(root, `public/og/${locale}.png`));
}
console.log(
  'Generated bilingual 1200×630 OG cards, icons, and public screenshot images.',
);
