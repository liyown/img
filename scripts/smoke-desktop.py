"""Start a real native window with disposable data and require a clean exit."""
from pathlib import Path
import os, platform, subprocess, tempfile
root=Path(__file__).resolve().parent.parent
system=platform.system()
if system=='Windows': binary=root/'target/desktop/windows-x86_64/img-desktop.exe'
elif system=='Linux': binary=root/'target/desktop/linux-x86_64/img-desktop'
else:
    arch='arm64' if platform.machine()=='arm64' else 'x86_64'
    binary=root/f'target/desktop/{arch}/Img.app/Contents/MacOS/img-desktop'
with tempfile.TemporaryDirectory(prefix='img-window-smoke-') as temp:
    temp=Path(temp)
    (temp/'queue.json').write_text('[]')
    (temp/'shortcuts.json').write_text('{"enabled":false}')
    (temp/'preferences.json').write_text('{"check_updates":false}')
    (temp/'config.toml').write_text('version=1\n')
    env=dict(os.environ,APERTURE_DATA_DIR=str(temp),APERTURE_CONFIG_PATH=str(temp/'config.toml'),APERTURE_HOTKEY_DIR=str(temp/'hotkeys'))
    result=subprocess.run([str(binary),'--smoke-test'],env=env,capture_output=True,text=True,timeout=45)
    print(result.stdout, result.stderr)
    assert result.returncode==0, 'Native window failed to start/render/exit'
    print(system+': native window smoke passed with isolated data')
