import { test } from 'node:test';
import assert from 'node:assert/strict';
import { latestDesktop } from '../src/lib/releases.mjs';
function release(version, arch = 'arm64') {
  const tag_name = `desktop-v${version}`;
  const name = `img-desktop_${version}_macos_${arch}.dmg`;
  return {
    tag_name,
    draft: false,
    prerelease: false,
    assets: [name, name + '.sha256'].map((name) => ({
      name,
      size: 100,
      browser_download_url: `https://github.com/liyown/img/releases/download/${tag_name}/${name}`,
    })),
  };
}
test('latest desktop uses semantic version ordering and ignores CLI, drafts and prereleases', () => {
  const data = [
    release('0.9.0'),
    release('0.10.0'),
    { ...release('9.0.0'), draft: true },
    { ...release('8.0.0'), prerelease: true },
    { ...release('7.0.0'), tag_name: 'v7.0.0' },
  ];
  assert.equal(latestDesktop(data).version, '0.10.0');
});
test('new releases change download targets without editing the website', () => {
  assert.match(
    latestDesktop([release('0.3.0')]).downloads.arm64,
    /desktop-v0\.3\.0/,
  );
  assert.match(
    latestDesktop([release('0.4.0'), release('0.3.0')]).downloads.arm64,
    /desktop-v0\.4\.0/,
  );
});
test('rejects missing checksums and foreign download URLs; never invents an Intel asset', () => {
  const invalid = release('0.4.0');
  invalid.assets[0].browser_download_url = 'https://evil.test/app.dmg';
  assert.equal(latestDesktop([invalid]), null);
  const incomplete = release('0.4.0');
  incomplete.assets.pop();
  assert.equal(latestDesktop([incomplete]), null);
  assert.equal(latestDesktop([release('0.3.0')]).downloads.x86_64, undefined);
  assert.equal(latestDesktop([]), null);
});

test('CLI latest is independent and excludes retired Go versions', async () => {
  const { latestCLI } = await import('../src/lib/releases.mjs');
  const cli = (version) => ({
    tag_name: `v${version}`,
    draft: false,
    prerelease: false,
    assets: [
      'checksums.txt',
      'img_windows_amd64.zip',
      'img_linux_arm64.tar.gz',
    ].map((name) => ({
      name,
      size: 100,
      browser_download_url: `https://github.com/liyown/img/releases/download/v${version}/${name}`,
    })),
  });
  assert.equal(latestCLI([cli('0.2.0')]), null);
  assert.equal(latestCLI([release('8.0.0'), cli('0.3.0')]).version, '0.3.0');
  assert.equal(latestCLI([cli('0.3.0'), cli('0.4.0')]).version, '0.4.0');
});
