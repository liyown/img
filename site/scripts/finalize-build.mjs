import { copyFile } from 'node:fs/promises';
// GitHub Pages only uses the project-root 404.html as its missing-page fallback.
await copyFile(
  new URL('../dist/404/index.html', import.meta.url),
  new URL('../dist/404.html', import.meta.url),
);
