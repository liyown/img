export const evidence = {
  checked: '2026-09-07',
  platform: 'macOS ARM64',
  package: 'GUI DMG',
  unit: 'MB (1 MB = 1,000,000 bytes)',
  products: [
    {
      name: 'img',
      version: '0.3.0',
      bytes: 26268008,
      source: 'https://github.com/liyown/img/releases/tag/desktop-v0.3.0',
      kind: 'official-release',
    },
    {
      name: 'PicGo',
      version: '3.0.2',
      bytes: 159758553,
      source:
        'https://github.com/Molunerfinn/PicGo/releases/download/v3.0.2/latest-mac.yml',
      kind: 'official-manifest',
    },
    {
      name: 'PicList',
      version: '3.5.0',
      bytes: 148617617,
      source:
        'https://github.com/Kuingsmile/PicList/releases/download/v3.5.0/latest-mac.yml',
      kind: 'official-manifest',
    },
    {
      name: 'uPic',
      version: 'App Store',
      bytes: null,
      source: 'https://github.com/gee1k/uPic',
      kind: 'unavailable',
    },
  ],
  cli: {
    bytes: 7088101,
    platform: 'macOS ARM64',
    package: 'tar.gz',
    version: '0.3.0',
  },
  benchmark: {
    version: '0.3.0',
    machine: 'Apple M4 · 16 GiB · macOS 27.0 (26A5388g)',
    records: 10000,
    searchP95Ms: 0.039167,
    cpuDrawP95Ms: 3.651583,
    samples: 299,
    cacheBytes: 67108864,
    source: 'https://github.com/liyown/img/blob/main/stability-qa.md',
  },
};
