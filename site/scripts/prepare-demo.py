#!/usr/bin/env python3
"""Prepare an isolated screenshot app from the current local release package.
Never opens a real provider, changes user configuration, or uploads an image.
Run from the repository root, then launch the printed command.
"""
import hashlib, json, os, plistlib, shutil, subprocess, time, uuid, zipfile
from pathlib import Path
repo = Path(__file__).resolve().parents[2]
root = repo / 'target/site-demo'
root.mkdir(parents=True, exist_ok=True)
package = repo / 'dist/desktop/arm64/img-desktop_0.3.0_macos_arm64.zip'
with zipfile.ZipFile(package) as archive:
    for entry in archive.infolist():
        if not any(p.startswith('._') for p in Path(entry.filename).parts):
            archive.extract(entry, root)
app = root / 'Img.app'
for executable in (app / 'Contents/MacOS').iterdir():
    executable.chmod(0o755)
info_path = app / 'Contents/Info.plist'
info = plistlib.loads(info_path.read_bytes())
info.update(CFBundleIdentifier='dev.img.site-demo', CFBundleName='img Website Demo', CFBundleDisplayName='img Website Demo')
info_path.write_bytes(plistlib.dumps(info))
subprocess.run(['codesign', '--force', '--sign', '-', str(app)], check=True)
data = root / 'data'
data.mkdir(exist_ok=True)
(data / 'preferences.json').write_text(json.dumps(dict(check_updates=False, auto_copy=False)))
(data / 'shortcuts.json').write_text('{"enabled":false}')
(root / 'config.toml').write_text("version = 1\ndefault_provider = 'Demo Storage'\n[providers.'Demo Storage']\ntype = 'http'\nurl = 'http://127.0.0.1:9/upload'\nurl_json_path = 'url'\nallow_insecure = true\n")
assets = [('assets/demo/mountain-sunset.webp','Mountain light.webp'), ('assets/demo/forest-light.png','Forest morning.png'), ('assets/demo/coastline-drone.jpg','Coastline.jpg'), ('desktop/assets/queue-coast.png','Blue hour.png'), ('desktop/assets/queue-bottle.png','Everyday objects.png')]
items=[]
for index in range(8):
    origin, name = assets[index % len(assets)]
    identity = str(uuid.uuid5(uuid.NAMESPACE_URL, f'img-website-demo-{index}'))
    folder = data / 'images' / identity
    folder.mkdir(parents=True, exist_ok=True)
    source = folder / name
    shutil.copyfile(repo / origin, source)
    preview = folder / 'preview.png'
    subprocess.run(['node', '--input-type=module', '-e', "import sharp from 'sharp'; await sharp(process.argv[1]).resize(1200,1200,{fit:'inside',withoutEnlargement:true}).png().toFile(process.argv[2]);", str(source), str(preview)], cwd=repo/'site', check=True)
    done = index >= 2
    items.append(dict(id=identity, name=name, size=source.stat().st_size, target='Demo Storage', source=str(source), thumbnail=str(preview), asset='', status='Done' if done else 'Ready', progress=100 if done else None, url=f'https://images.example.com/demo/{index}.png' if done else None, error=None, simulated=False, added_at=int(time.time())-index*900))
(data / 'queue.json').write_text(json.dumps(items))
env = {k:v for k,v in os.environ.items() if not k.startswith(('APERTURE_', 'IMG_'))}
env.update(APERTURE_DATA_DIR=str(data), APERTURE_CONFIG_PATH=str(root/'config.toml'), APERTURE_APP_ID='dev.img.site-demo', APERTURE_HOTKEY_DIR=str(root/'hotkeys'))
(root / 'provenance.json').write_text(json.dumps(dict(version='0.3.0', product_commit='cec6219', package_sha256=hashlib.sha256(package.read_bytes()).hexdigest(), desktop_sha256=hashlib.sha256((app/'Contents/MacOS/img-desktop').read_bytes()).hexdigest(), demo_records=8, remote_uploads=0), indent=2)+'\n')
log = (root/'demo.log').open('w')
process = subprocess.Popen([str(app/'Contents/MacOS/img-desktop')], cwd=root, env=env, stdout=log, stderr=log)
print(f'Isolated demo app PID: {process.pid}')

process.wait()
