const repository = 'https://github.com/liyown/img';
/** Resolve immutable assets from the newest stable desktop release, independent of CLI latest.
 * @param {unknown} value
 * @returns {{version:string,page:string,downloads:Record<string,string>} | null}
 */
export function latestDesktop(value) {
  if (!Array.isArray(value)) throw new Error('Invalid release response');
  const releases = value.filter(
    (r) =>
      !r.draft && !r.prerelease && /^desktop-v\d+\.\d+\.\d+$/.test(r.tag_name),
  );
  releases.sort((a, b) => {
    const av = a.tag_name.slice(9).split('.').map(Number);
    const bv = b.tag_name.slice(9).split('.').map(Number);
    return bv[0] - av[0] || bv[1] - av[1] || bv[2] - av[2];
  });
  for (const release of releases) {
    const version = release.tag_name.slice(9);
    const base = `${repository}/releases/download/${release.tag_name}/`;
    const downloads = {};
    for (const [key, platform, arch, ext] of [
      ['arm64', 'macos', 'arm64', 'dmg'],
      ['x86_64', 'macos', 'x86_64', 'dmg'],
      ['windows', 'windows', 'x86_64', 'exe'],
      ['linux', 'linux', 'x86_64', 'deb'],
    ]) {
      const name = `img-desktop_${version}_${platform}_${arch}.${ext}`;
      const asset = release.assets?.find(
        (a) =>
          a.name === name &&
          a.size > 0 &&
          a.browser_download_url === base + name,
      );
      const checksum = release.assets?.find(
        (a) =>
          a.name === name + '.sha256' &&
          a.size > 0 &&
          a.browser_download_url === base + name + '.sha256',
      );
      if (asset && checksum) downloads[key] = asset.browser_download_url;
    }
    if (Object.keys(downloads).length)
      return {
        version,
        page: `${repository}/releases/tag/${release.tag_name}`,
        downloads,
      };
  }
  return null;
}

/** @param {unknown} value @returns {{version:string,page:string,downloads:Record<string,string>} | null} */
export function latestCLI(value) {
  if (!Array.isArray(value)) throw new Error('Invalid release response');
  const releases = value.filter(
    (r) => !r.draft && !r.prerelease && /^v\d+\.\d+\.\d+$/.test(r.tag_name),
  );
  releases.sort((a, b) => {
    const av = a.tag_name.slice(1).split('.').map(Number),
      bv = b.tag_name.slice(1).split('.').map(Number);
    return bv[0] - av[0] || bv[1] - av[1] || bv[2] - av[2];
  });
  for (const release of releases) {
    const version = release.tag_name.slice(1);
    const [major, minor] = version.split('.').map(Number);
    if (major === 0 && minor < 3) continue; // Earlier releases are the retired Go implementation.
    const base = `${repository}/releases/download/${release.tag_name}/`;
    if (
      !release.assets?.some(
        (a) =>
          a.name === 'checksums.txt' &&
          a.size > 0 &&
          a.browser_download_url === base + 'checksums.txt',
      )
    )
      continue;
    const downloads = {};
    for (const name of [
      'img_darwin_arm64.tar.gz',
      'img_darwin_amd64.tar.gz',
      'img_linux_amd64.tar.gz',
      'img_linux_arm64.tar.gz',
      'img_windows_amd64.zip',
    ]) {
      const asset = release.assets.find(
        (a) =>
          a.name === name &&
          a.size > 0 &&
          a.browser_download_url === base + name,
      );
      if (asset) downloads[name] = asset.browser_download_url;
    }
    if (Object.keys(downloads).length)
      return {
        version,
        page: `${repository}/releases/tag/${release.tag_name}`,
        downloads,
      };
  }
  return null;
}
