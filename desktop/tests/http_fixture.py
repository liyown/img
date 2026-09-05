"""Local-only, controllable upload endpoint for native desktop acceptance checks."""
import argparse, http.server, json, pathlib, struct, threading, time, zlib, random

parser = argparse.ArgumentParser()
parser.add_argument('directory', type=pathlib.Path)
args = parser.parse_args()
root = args.directory
root.mkdir(parents=True, exist_ok=True)
state = {'delay': 0, 'fail': False, 'requests': [], 'active': 0, 'peak': 0}
lock = threading.Lock()

class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'
    def log_message(self, *args): pass
    def reply(self, status, body=b'', content_type='application/json'):
        self.send_response(status)
        self.send_header('Content-Length', str(len(body)))
        self.send_header('Content-Type', content_type)
        self.end_headers()
        if self.command != 'HEAD': self.wfile.write(body)
    def do_HEAD(self): self.reply(200)
    def do_GET(self):
        if self.path == '/state':
            with lock: data = json.dumps(state).encode()
            self.reply(200, data)
        elif self.path == '/image.png': self.reply(200, (root/'photo-1.png').read_bytes(), 'image/png')
        else: self.reply(404)
    def do_POST(self):
        length = int(self.headers.get('Content-Length', 0))
        if self.path == '/control':
            changes = json.loads(self.rfile.read(length))
            with lock: state.update(changes)
            self.reply(200, b'{}'); return
        with lock:
            state['active'] += 1
            state['peak'] = max(state['peak'], state['active'])
            delay, fail = state['delay'], state['fail']
            event = {'length': length, 'received': 0, 'method': self.command}
            state['requests'].append(event)
            index = len(state['requests'])
        try:
            while event['received'] < length:
                block = self.rfile.read(min(16384, length-event['received']))
                if not block: break
                event['received'] += len(block)
                if delay: time.sleep(delay)
            if fail: self.reply(503, b'{"error":"fixture failure"}')
            else: self.reply(200, json.dumps({'data': {'url': f'https://cdn.example.test/photo-{index}.png'}}).encode())
        except (BrokenPipeError, ConnectionResetError): event['interrupted'] = True
        finally:
            with lock: state['active'] -= 1
    do_PUT = do_POST
    do_PATCH = do_POST

def png(seed):
    width, height = 900, 700
    randomizer = random.Random(seed)
    data = b''.join(b'\0'+randomizer.randbytes(width*3) for _ in range(height))
    def chunk(typ, body): return struct.pack('>I', len(body))+typ+body+struct.pack('>I', zlib.crc32(typ+body)&0xffffffff)
    return b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 2, 0, 0, 0))+chunk(b'IDAT', zlib.compress(data))+chunk(b'IEND', b'')
for number in range(1, 5): (root/f'photo-{number}.png').write_bytes(png(number))
(root/'vector.svg').write_text('<svg xmlns="http://www.w3.org/2000/svg" width="360" height="180"><rect width="360" height="180" fill="#f3e4cf"/><circle cx="180" cy="90" r="50" fill="#d46f36"/></svg>')
server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
server.daemon_threads = True
(root/'port').write_text(str(server.server_port))
print(f'Local fixture: http://127.0.0.1:{server.server_port}', flush=True)
server.serve_forever()
