"""Extract and verify a platform GUI package without touching user installs."""
from pathlib import Path
import hashlib, os, platform, re, subprocess, tempfile, zipfile
root=Path(__file__).resolve().parent.parent
windows=platform.system()=='Windows'
osname='windows' if windows else 'linux'
version=re.search(r'^version = "([^"]+)"',(root/'Cargo.toml').read_text(),re.M)[1]
packages=root/'dist/desktop'/(osname+'-x86_64')
for checksum in packages.glob('*.sha256'):
    digest,name=checksum.read_text().split()
    assert hashlib.sha256((packages/name).read_bytes()).hexdigest()==digest
with tempfile.TemporaryDirectory(prefix='img-desktop-test-') as tmp:
    tmp=Path(tmp)
    if windows:
        with zipfile.ZipFile(next(packages.glob('*.zip'))) as archive: archive.extractall(tmp)
        binaries=tmp
        # The EXE installer must also support first install and overwrite without launching UI.
        exe=next(packages.glob('*.exe'))
        for _ in range(2):
            subprocess.run([str(exe),'/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART','/SP-',f'/DIR={tmp / "installed"}'],check=True,timeout=60)
        assert (tmp/'installed/img-desktop.exe').is_file()
    else:
        subprocess.run(['dpkg-deb','-x',str(next(packages.glob('*.deb'))),str(tmp)],check=True)
        binaries=tmp/'usr/lib/img'
        assert (tmp/'usr/share/applications/img.desktop').is_file()
    suffix='.exe' if windows else ''
    assert (binaries/('img-desktop'+suffix)).is_file()
    out=subprocess.check_output([str(binaries/('img'+suffix)),'version'],text=True)
    assert f'img {version}' in out and 'implementation: Rust' in out
    print(osname+': native GUI artifact, bundled CLI version, and checksums verified')
