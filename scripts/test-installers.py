"""Exercise built release packages locally, without GUI access or cloud credentials.

Usage: python3 scripts/test-installers.py [--cli-only]
Only temporary installation directories and a loopback HTTP endpoint are used.
"""
from pathlib import Path
import argparse
import re
import hashlib
import http.server
import json
import os
import platform
import shutil
import struct
import subprocess
import tempfile
import threading
import zlib

ROOT = Path(__file__).resolve().parent.parent
args = argparse.ArgumentParser()
group = args.add_mutually_exclusive_group()
group.add_argument('--cli-only', action='store_true')
group.add_argument('--gui-only', action='store_true')
opts = args.parse_args()
arch = 'aarch64' if platform.machine() in ('arm64', 'aarch64') else 'x86_64'
system = platform.system()
if system == 'Darwin':
    target = f'{arch}-apple-darwin'
elif system == 'Linux':
    target = f'{arch}-unknown-linux-gnu'
elif system == 'Windows':
    target = 'x86_64-pc-windows-msvc'
else:
    raise SystemExit('Unsupported installer test host')
powershell = shutil.which('pwsh') or shutil.which('powershell')
installer = ([powershell, '-NoProfile', '-File', str(ROOT / 'install.ps1'), '-Product']
             if system == 'Windows' else ['/bin/sh', str(ROOT / 'install.sh')])
def install_args(product):
    return installer + ([product, '-NoPathUpdate'] if system == 'Windows' else ['--' + product])

gui_arch = 'arm64' if arch == 'aarch64' else 'x86_64'
version = re.search(r'^version = "([^"]+)"', (ROOT / 'Cargo.toml').read_text(), re.M)[1]
products = ['cli'] if opts.cli_only else ['gui'] if opts.gui_only else ['cli', 'gui']
if system != 'Darwin' and products != ['cli']:
    raise SystemExit('Use --cli-only outside macOS')


def run(argv, env, cwd, success=True):
    result = subprocess.run(argv, env=env, cwd=cwd, capture_output=True, text=True, timeout=30)
    if success and result.returncode:
        raise AssertionError(f'{argv[0]} failed: {result.stderr}\n{result.stdout}')
    if not success and result.returncode == 0:
        raise AssertionError('corrupted archive was unexpectedly accepted')
    return result


def png():
    def chunk(kind, body):
        return struct.pack('>I', len(body)) + kind + body + struct.pack('>I', zlib.crc32(kind + body) & 0xffffffff)
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', 16, 8, 8, 2, 0, 0, 0)) + chunk(b'IDAT', zlib.compress((b'\0' + b'\x30\x80\xc0' * 16) * 8)) + chunk(b'IEND', b'')


class Handler(http.server.BaseHTTPRequestHandler):
    received = []

    def log_message(self, *_):
        pass

    def do_POST(self):
        data = self.rfile.read(int(self.headers['Content-Length']))
        self.received.append(data)
        body = b'{"data":{"url":"https://cdn.example.test/package.png"}}'
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)


with tempfile.TemporaryDirectory(prefix='img-rust-installer-') as tmp:
    tmp = Path(tmp)
    env = {k: v for k, v in os.environ.items() if not k.startswith(('IMG_', 'APERTURE_'))}
    # Installed products must work with no Rust, Go or source checkout on PATH.
    test_path = (os.path.join(os.environ['SystemRoot'], 'System32') + os.pathsep + str(Path(powershell).parent)
                 if system == 'Windows' else '/usr/bin:/bin:/usr/sbin:/sbin')
    env.update(PATH=test_path, IMG_VERSION=version)
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    worker = threading.Thread(target=server.serve_forever, daemon=True)
    worker.start()
    try:
        for product in products:
            package_dir = ROOT / ('dist/cli/' + target if product == 'cli' else 'dist/desktop/' + gui_arch)
            install_dir = tmp / (product + ' command directory')
            app_dir = tmp / 'Applications'
            install_env = dict(env, IMG_LOCAL_PACKAGE_DIR=str(package_dir), IMG_INSTALL_DIR=str(install_dir), IMG_APP_DIR=str(app_dir))
            result = run(install_args(product), install_env, tmp)
            binary = install_dir / ('img.exe' if system == 'Windows' else 'img')
            assert binary.is_file()
            version_output = run([str(binary), 'version'], env, tmp).stdout
            assert f'img {version}' in version_output and 'implementation: Rust' in version_output
            if product == 'gui':
                assert binary.is_symlink()
                assert binary.resolve() == (app_dir / 'Img.app/Contents/MacOS/img').resolve()
                assert (app_dir / 'Img.app/Contents/MacOS/img-desktop').is_file()
                assert not (app_dir / 'Img.app/Contents/MacOS/img-engine').exists()
            else:
                assert not binary.is_symlink()
                assert not app_dir.exists()
            source = tmp / 'image.png'
            source.write_bytes(png())
            config = tmp / (product + '.toml')
            config.write_text(f"version=1\ndefault_provider='local'\n[upload]\npath_template='{{filename}}'\n[providers.local]\ntype='http'\nurl='http://127.0.0.1:{server.server_port}/upload'\nurl_json_path='data.url'\nallow_insecure=true\n")
            before = hashlib.sha256(config.read_bytes()).hexdigest()
            uploaded = run([str(binary), '--config', str(config), 'upload', str(source), '--format', 'json', '--no-copy', '--progress'], env, tmp)
            assert json.loads(uploaded.stdout)['files'][0]['url'] == 'https://cdn.example.test/package.png'
            events = [json.loads(line) for line in uploaded.stderr.splitlines()]
            assert events[-1]['stage'] == 'waiting'
            assert events[-1]['sent'] == events[-1]['total'] == len(Handler.received[-1])
            assert source.read_bytes() == png()
            assert hashlib.sha256(config.read_bytes()).hexdigest() == before
            # Replace an existing installation while preserving representative app state.
            state = tmp / ('data-' + product)
            state.mkdir()
            sentinels = {'queue.json': b'[{"id":"keep"}]', 'preferences.json': b'{"auto_copy":true}',
                         'original.png': png(), 'config.toml': config.read_bytes()}
            for name, content in sentinels.items(): (state / name).write_bytes(content)
            run(install_args(product), dict(install_env, APERTURE_DATA_DIR=str(state), APERTURE_CONFIG_PATH=str(config)), tmp)
            for name, content in sentinels.items(): assert (state / name).read_bytes() == content
            assert hashlib.sha256(config.read_bytes()).hexdigest() == before
            assert f'img {version}' in run([str(binary), 'version'], env, tmp).stdout
            print(f'{product}: first and overwrite install, Rust version, local upload, progress and data retention verified without toolchains')
        for product in products:
            broken = tmp / ('corrupted-' + product)
            broken.mkdir()
            source_dir = ROOT / ('dist/cli/' + target if product == 'cli' else 'dist/desktop/' + gui_arch)
            for file in source_dir.iterdir():
                if file.is_file():
                    shutil.copyfile(file, broken / file.name)
            archive = next(broken.glob(('*.zip' if system == 'Windows' else '*.tar.gz') if product == 'cli' else f'img-desktop_{version}_*.zip'))
            with archive.open('ab') as file:
                file.write(b'tampered')
            bad_install = tmp / ('must remain empty ' + product)
            run(install_args(product), dict(env, IMG_INSTALL_DIR=str(bad_install), IMG_APP_DIR=str(tmp / 'must not install apps'), IMG_LOCAL_PACKAGE_DIR=str(broken)), tmp, success=False)
            assert not (bad_install / ('img.exe' if system == 'Windows' else 'img')).exists()
            existing = tmp / (product + ' command directory')
            binary = existing / ('img.exe' if system == 'Windows' else 'img')
            digest = hashlib.sha256(binary.read_bytes()).hexdigest()
            run(install_args(product), dict(env, IMG_INSTALL_DIR=str(existing), IMG_APP_DIR=str(tmp / 'Applications'), IMG_LOCAL_PACKAGE_DIR=str(broken)), tmp, success=False)
            assert hashlib.sha256(binary.read_bytes()).hexdigest() == digest
            print(f'tampered {product} archive: rejected for first install and upgrade; existing executable retained')
    finally:
        server.shutdown()
        server.server_close()
        worker.join()
