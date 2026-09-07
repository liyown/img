#!/usr/bin/env python3
"""Run native release benchmarks with isolated generated fixtures.

Build first: cargo build --locked --release -p img-desktop --features perf
The baseline replays full element construction and large previews on the current
framework/layout. It is not a checkout of the previous binary. The 10k baseline
is opt-in because it exceeded 90 seconds on the 16 GiB development machine.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import plistlib
import shutil
import struct
import subprocess
import uuid
import zlib

parser = argparse.ArgumentParser()
parser.add_argument('--heavy-baseline', action='store_true', help='Also run the expensive 10,000-row baseline')
mode = parser.add_mutually_exclusive_group()
mode.add_argument('--optimized-only', action='store_true')
mode.add_argument('--baseline-only', action='store_true')
parser.add_argument('--records', type=int, choices=[1000, 10000], help='Measure only this fixture size')
opts = parser.parse_args()
repo = Path(__file__).resolve().parent.parent
base = repo / 'target/stability-perf'
base.mkdir(exist_ok=True)
binary = repo / 'target/release/img-desktop'
if not binary.is_file():
    raise SystemExit('Build img-desktop with --release --features perf first')
# Run as a real macOS application, matching the distributed GUI lifecycle.
# Bare executables can lack an activatable AppKit application/window.
bundle = base / 'Benchmark.app'
contents = bundle / 'Contents'
(contents / 'MacOS').mkdir(parents=True, exist_ok=True)
shutil.copy2(binary, contents / 'MacOS/img-desktop')
with (contents / 'Info.plist').open('wb') as info:
    plistlib.dump(dict(CFBundleIdentifier='dev.img.benchmark', CFBundleName='img Benchmark',
                       CFBundleExecutable='img-desktop', CFBundlePackageType='APPL',
                       CFBundleVersion='1', CFBundleShortVersionString='0.3.0',
                       LSMinimumSystemVersion='13.0', NSHighResolutionCapable=True), info)
subprocess.run(['/usr/bin/codesign', '--force', '--sign', '-', str(bundle)], check=True,
               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
binary = contents / 'MacOS/img-desktop'


def chunk(kind, data):
    return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))


width, height = 1200, 900
raw = b''.join(b'\0' + b''.join(bytes([x % 256, y % 256, 180]) for x in range(width)) for y in range(height))
(base / 'sample.png').write_bytes(b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 2, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(raw)) + chunk(b'IEND', b''))
(base / 'config.toml').write_text("version=1\ndefault_provider='local'\n[providers.local]\ntype='http'\nurl='http://127.0.0.1:9/upload'\nurl_json_path='url'\nallow_insecure=true\n")
for count in [1000, 10000]:
    root = base / str(count)
    root.mkdir(exist_ok=True)
    (root / '.img-perf-fixture').touch()
    (root / 'shortcuts.json').write_text('{"enabled":false}')
    (root / 'preferences.json').write_text('{"check_updates":false}')
    items = []
    for index in range(count):
        identity = str(uuid.uuid5(uuid.NAMESPACE_DNS, f'img-perf-{index}'))
        folder = root / 'images' / identity
        folder.mkdir(parents=True, exist_ok=True)
        preview = folder / 'preview.png'
        if not preview.exists():
            os.link(base / 'sample.png', preview)
        items.append(dict(id=identity, name=f'photo-{index:05}.png', size=100000, target='local', source=None,
                          thumbnail=str(preview), asset='', status='Done', progress=100,
                          url=f'https://example.test/photo-{index:05}.png', error=None, simulated=False, added_at=1))
    (root / 'queue.json').write_text(json.dumps(items))

cases = [(1000, False), (10000, False)]
if not opts.optimized_only:
    cases.append((1000, True))
    if opts.heavy_baseline:
        cases.append((10000, True))
if opts.records:
    cases = [case for case in cases if case[0] == opts.records]
if opts.baseline_only:
    cases = [case for case in cases if case[1]]
if not cases:
    raise SystemExit('The 10,000-record baseline requires --heavy-baseline')
for count, baseline in cases:
    name = f'{count}-' + ('baseline' if baseline else 'virtual')
    output = base / f'{name}.json'
    output.unlink(missing_ok=True)
    env = {key: value for key, value in os.environ.items() if not key.startswith(('IMG_PERF_', 'APERTURE_'))}
    env.update(APERTURE_DATA_DIR=str(base / str(count)), APERTURE_CONFIG_PATH=str(base / 'config.toml'),
               APERTURE_APP_ID='dev.img.benchmark', APERTURE_HOTKEY_DIR=str(base / 'hotkeys'),
               IMG_PERF_OUTPUT=str(output))
    if baseline:
        env['IMG_PERF_BASELINE'] = '1'
    print(f'Starting {name}: keep the test window active and visible during warmup and measurement.', flush=True)
    with (base / f'{name}.log').open('w') as log:
        process = subprocess.Popen([str(binary)], env=env, cwd=base, stdout=log, stderr=log)
        try:
            process.wait(timeout=90)
        except subprocess.TimeoutExpired:
            process.terminate()
            try:
                process.wait(timeout=8)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
            output.write_text(json.dumps(dict(records=count, baseline=baseline, timed_out=True)))
    if not output.exists():
        raise SystemExit(f'{name} did not produce a report; inspect {name}.log')
    report = json.loads(output.read_text())
    report['binary_sha256'] = hashlib.sha256(binary.read_bytes()).hexdigest()
    output.write_text(json.dumps(report, indent=2) + '\n')
    print(name, report, flush=True)
    if report.get('timed_out'):
        raise SystemExit(f'{name} timed out; performance acceptance is incomplete')
    assert report.get('warmup_ready'), 'No active native frame stream; keep the benchmark window visible'
    assert process.returncode == 0 and report['draw_samples'] >= 30
    if not baseline:
        assert report['search_p95_ms'] < 100
        assert report['draw_p95_ms'] <= 16.7
        assert report['thumbnail_cache_peak_bytes'] <= 64 * 1024 * 1024
