#!/usr/bin/env python3
"""Native release benchmark, isolated fixtures; build with --release --features perf first.
The controlled baseline uses full record cloning, full element trees and large previews.
It shares the current layout/framework; it is not a checkout of the previous binary.
"""
import pathlib, json, uuid, os, struct, zlib
repo=pathlib.Path(__file__).resolve().parent.parent
base=repo/'target/stability-perf' 
base.mkdir(exist_ok=True)
def chunk(t,d): return struct.pack('>I',len(d))+t+d+struct.pack('>I',zlib.crc32(t+d))
w,h=1200,900
raw=b''.join(b'\0'+b''.join(bytes([x%256,y%256,180]) for x in range(w)) for y in range(h))
(base/'sample.png').write_bytes(b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',w,h,8,2,0,0,0))+chunk(b'IDAT',zlib.compress(raw))+chunk(b'IEND',b''))
(base/'config.toml').write_text('[defaults]\nprovider = "local"\n[providers.local]\ntype = "http"\nendpoint = "http://127.0.0.1:9/upload"\nurl_path = "url"\n')
for n in [1000,10000]:
 root=base/str(n); root.mkdir(exist_ok=True); (root/'.img-perf-fixture').touch()
 items=[]
 for i in range(n):
  uid=str(uuid.uuid5(uuid.NAMESPACE_DNS,f'img-perf-{i}')); folder=root/'images'/uid;folder.mkdir(parents=True,exist_ok=True)
  preview=folder/'preview.png'
  if not preview.exists(): os.link(base/'sample.png',preview)
  items.append(dict(id=uid,name=f'photo-{i:05}.png',size=100000,target='local',source=None,thumbnail=str(preview),asset='',status='Done',progress=100,url=f'https://example.test/photo-{i:05}.png',error=None,simulated=False,added_at=1))
 (root/'queue.json').write_text(json.dumps(items))
print(base)

import os, subprocess, pathlib, json, time

for count,baseline in [(1000,False),(10000,False),(1000,True),(10000,True)]:
 name=f'{count}-'+('baseline' if baseline else 'virtual')
 env=os.environ.copy();env.update(APERTURE_DATA_DIR=str(base/str(count)),APERTURE_CONFIG_PATH=str(base/'config.toml'),IMG_PERF_OUTPUT=str(base/f'{name}.json'))
 if baseline:env['IMG_PERF_BASELINE']='1'
 with (base/f'{name}.log').open('w') as log:
  p=subprocess.Popen([str(repo/'target/release/img-desktop')],env=env,cwd='/tmp',stdout=log,stderr=log)
  try:p.wait(timeout=90)
  except subprocess.TimeoutExpired:
   p.terminate()
   try:p.wait(timeout=8)
   except subprocess.TimeoutExpired:p.kill();p.wait()
   (base/f'{name}.json').write_text(json.dumps(dict(records=count,baseline=baseline,timed_out=True)))
 print(name,p.returncode,flush=True)
 if (base/f'{name}.json').exists():print((base/f'{name}.json').read_text(),flush=True)
