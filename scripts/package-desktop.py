"""Build native Windows/Linux GUI packages on their CI runners."""
from pathlib import Path
import hashlib, json, os, platform, re, shutil, subprocess, zipfile
root = Path(__file__).resolve().parent.parent
os.chdir(root)
version = re.search(r'^version = "([^"]+)"', (root/'Cargo.toml').read_text(), re.M)[1]
if os.environ.get('GITHUB_REF_TYPE') == 'tag':
    assert os.environ['GITHUB_REF_NAME'] == 'desktop-v' + version
windows = platform.system() == 'Windows'
assert platform.system() in ('Windows', 'Linux')
assert platform.machine().lower() in ('amd64','x86_64')
osname = 'windows' if windows else 'linux'
target = 'x86_64-pc-windows-msvc' if windows else 'x86_64-unknown-linux-gnu'
key = osname + '-x86_64'
output = root/'dist/desktop'/key
stage = root/'target/desktop'/key
output.mkdir(parents=True, exist_ok=True)
if stage.exists(): shutil.rmtree(stage)
stage.mkdir(parents=True)
env = dict(os.environ)
if windows: env['RUSTFLAGS'] = (env.get('RUSTFLAGS','') + ' -C target-feature=+crt-static').strip()
subprocess.run(['cargo','build','--locked','--release','-p','img-desktop','-p','img-cli','--target',target],env=env,check=True)
ext = '.exe' if windows else ''
for name in ['img-desktop','img']:
    shutil.copy2(root/'target'/target/'release'/(name+ext),stage/(name+ext))
shutil.copy2(root/'desktop/INSTALL.html',stage/'INSTALL.html')
shutil.copy2(root/'crates/img-core/assets/NotoSansCJK-LICENSE.txt',stage/'NotoSansCJK-LICENSE.txt')
name = f'img-desktop_{version}_{osname}_x86_64'
if windows:
    with zipfile.ZipFile(output/(name+'.zip'),'w',zipfile.ZIP_DEFLATED) as archive:
        for path in stage.iterdir(): archive.write(path,path.name)
    iscc = shutil.which('iscc') or str(Path(os.environ.get('ProgramFiles(x86)',r'C:\Program Files (x86)'))/'Inno Setup 6/ISCC.exe')
    subprocess.run([iscc,f'/DVersion={version}',str(root/'desktop/windows-installer.iss')],check=True)
else:
    deb = root/'target/deb-root'
    if deb.exists(): shutil.rmtree(deb)
    (deb/'usr/lib/img').mkdir(parents=True)
    (deb/'usr/bin').mkdir(parents=True)
    (deb/'usr/share/applications').mkdir(parents=True)
    (deb/'usr/share/icons/hicolor/256x256/apps').mkdir(parents=True)
    (deb/'DEBIAN').mkdir()
    for path in stage.iterdir(): shutil.copy2(path,deb/'usr/lib/img'/path.name)
    for binary in ['img-desktop','img']:
        (deb/'usr/bin'/binary).symlink_to('../lib/img/'+binary)
    shutil.copy2(root/'desktop/assets/img-mark.png',deb/'usr/share/icons/hicolor/256x256/apps/img.png')
    (deb/'usr/share/applications/img.desktop').write_text('[Desktop Entry]\nType=Application\nName=img\nComment=Upload images and copy links\nExec=/usr/bin/img-desktop\nIcon=img\nTerminal=false\nCategories=Graphics;Utility;\n')
    (deb/'DEBIAN/control').write_text(f'Package: img-desktop\nVersion: {version}\nArchitecture: amd64\nMaintainer: img community <noreply@github.com>\nDepends: libc6 (>= 2.39), libgcc-s1, libstdc++6, libx11-6, libxcb1, libxkbcommon0, libxkbcommon-x11-0, libwayland-client0, libwayland-cursor0, libvulkan1, libfontconfig1, libdbus-1-3, curl, xdg-utils, libpolkit-agent-1-0, pkexec\nRecommends: gnome-keyring, gnome-screenshot\nDescription: Native image uploader with bundled CLI\n')
    subprocess.run(['dpkg-deb','--root-owner-group','--build',str(deb),str(output/(name+'.deb'))],check=True)
for package in output.iterdir():
    if package.suffix in ('.exe','.zip','.deb'):
        package.with_name(package.name+'.sha256').write_text(hashlib.sha256(package.read_bytes()).hexdigest()+'  '+package.name+'\n')
(output/'build-info.json').write_text(json.dumps(dict(version=version,platform=osname,arch='x86_64',channel='community',notarized=False),indent=2))
print(output)
