"""Start a real native window with disposable data and require a clean exit."""
from pathlib import Path
import json, os, platform, subprocess, tempfile
root=Path(__file__).resolve().parent.parent
system=platform.system()
if system=='Windows': binary=root/'target/desktop/windows-x86_64/img-desktop.exe'
elif system=='Linux': binary=root/'target/desktop/linux-x86_64/img-desktop'
else:
    arch='arm64' if platform.machine()=='arm64' else 'x86_64'
    binary=root/f'target/desktop/{arch}/Img.app/Contents/MacOS/img-desktop'
with tempfile.TemporaryDirectory(prefix='img-window-smoke-') as temp:
    base=Path(temp)
    temp=base/'data'
    temp.mkdir()
    (temp/'queue.json').write_text('[]')
    (temp/'shortcuts.json').write_text('{"enabled":false}')
    (temp/'preferences.json').write_text('{"check_updates":false}')
    (temp/'config.toml').write_text('version=1\n')
    env=dict(os.environ,IMG_DATA_DIR=str(temp),APERTURE_DATA_DIR=str(temp),APERTURE_CONFIG_PATH=str(temp/'config.toml'),APERTURE_HOTKEY_DIR=str(temp/'hotkeys'))
    result=subprocess.run([str(binary),'--smoke-test'],env=env,capture_output=True,text=True,timeout=45)
    print(result.stdout, result.stderr)
    assert result.returncode==0, 'Native window failed to start/render/exit'
    print(system+': native window smoke passed with isolated data')
    cli=binary.with_name('img.exe' if system=='Windows' else 'img')
    backup=base/'backup'
    exported=subprocess.run([str(cli),'--config',str(temp/'config.toml'),'backup',str(backup)],env=env,capture_output=True,text=True,timeout=45)
    assert exported.returncode==0, exported.stderr
    (temp/'preferences.json').write_text('{"english":true,"check_updates":false}')
    request=dict(source=str(backup),config=str(temp/'config.toml'),credentials=False,manifest=list((backup/'manifest.json').read_bytes()))
    (temp/'restore-ready.json').write_text(json.dumps(request))
    restored=subprocess.run([str(binary),'--smoke-test'],env=env,capture_output=True,text=True,timeout=45)
    assert restored.returncode==0, restored.stderr
    assert not (temp/'restore-ready.json').exists(), 'Restore request was not consumed'
    assert not json.loads((temp/'preferences.json').read_text()).get('english',False), 'Backup preferences were not restored'
    assert list(base.glob('img-before-restore-*')), 'Recovery copy missing'
    print(system+': backup, startup restore, recovery copy and native reopen passed')
