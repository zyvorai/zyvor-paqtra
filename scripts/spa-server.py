#!/usr/bin/env python3
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

"""SPA HTTP server with API/WebSocket reverse proxy to backend."""
import http.server
import http.client
import os
import sys
import socket
import hashlib
import base64
import threading
import struct

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 3001
DIRECTORY = sys.argv[2] if len(sys.argv) > 2 else '/var/lib/paqtra/ui'
API_HOST = os.environ.get('API_HOST', 'localhost')
API_PORT = int(os.environ.get('API_PORT', '9191'))

class SPAProxyHandler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=DIRECTORY, **kwargs)

    def do_GET(self):
        # WebSocket upgrade
        if self.path.startswith('/api/v1/ws/') and 'upgrade' in self.headers.get('Connection', '').lower():
            return self._proxy_websocket()
        # API proxy
        if self.path.startswith('/api/') or self.path in ('/health', '/ready', '/metrics'):
            return self._proxy_request('GET')
        # Static files
        file_path = os.path.join(DIRECTORY, self.path.lstrip('/'))
        if os.path.isfile(file_path):
            return super().do_GET()
        # SPA fallback
        self.path = '/index.html'
        return super().do_GET()

    def do_POST(self):
        if self.path.startswith('/api/'):
            return self._proxy_request('POST')
        self.send_error(404)

    def do_PUT(self):
        if self.path.startswith('/api/'):
            return self._proxy_request('PUT')
        self.send_error(404)

    def do_DELETE(self):
        if self.path.startswith('/api/'):
            return self._proxy_request('DELETE')
        self.send_error(404)

    def _proxy_request(self, method):
        try:
            conn = http.client.HTTPConnection(API_HOST, API_PORT, timeout=30)
            body = None
            if method in ('POST', 'PUT'):
                length = int(self.headers.get('Content-Length', 0))
                body = self.rfile.read(length) if length > 0 else None

            headers = {}
            for key in ('Content-Type', 'Authorization', 'Accept'):
                val = self.headers.get(key)
                if val:
                    headers[key] = val

            conn.request(method, self.path, body=body, headers=headers)
            resp = conn.getresponse()
            resp_body = resp.read()

            self.send_response(resp.status)
            for key, val in resp.getheaders():
                if key.lower() not in ('transfer-encoding', 'connection'):
                    self.send_header(key, val)
            # CORS
            self.send_header('Access-Control-Allow-Origin', '*')
            self.end_headers()
            self.wfile.write(resp_body)
            conn.close()
        except Exception as e:
            self.send_error(502, f'Backend unavailable: {e}')

    def _proxy_websocket(self):
        """Proxy WebSocket connection to backend."""
        try:
            # Connect to backend
            backend = socket.create_connection((API_HOST, API_PORT), timeout=5)

            # Forward the upgrade request
            ws_key = self.headers.get('Sec-WebSocket-Key', '')
            req_lines = [
                f'GET {self.path} HTTP/1.1',
                f'Host: {API_HOST}:{API_PORT}',
                'Upgrade: websocket',
                'Connection: Upgrade',
                f'Sec-WebSocket-Key: {ws_key}',
                'Sec-WebSocket-Version: 13',
                '', ''
            ]
            backend.sendall('\r\n'.join(req_lines).encode())

            # Read backend response
            resp_data = b''
            while b'\r\n\r\n' not in resp_data:
                chunk = backend.recv(4096)
                if not chunk:
                    break
                resp_data += chunk

            # Send response back to client
            self.wfile.write(resp_data)
            self.wfile.flush()

            # Bidirectional pipe
            client_sock = self.request
            client_sock.setblocking(False)
            backend.setblocking(False)

            def pipe(src, dst, name):
                try:
                    while True:
                        try:
                            data = src.recv(65536)
                            if not data:
                                break
                            dst.sendall(data)
                        except BlockingIOError:
                            import time
                            time.sleep(0.01)
                        except Exception:
                            break
                except Exception:
                    pass

            t1 = threading.Thread(target=pipe, args=(client_sock, backend, 'c2b'), daemon=True)
            t2 = threading.Thread(target=pipe, args=(backend, client_sock, 'b2c'), daemon=True)
            t1.start()
            t2.start()
            t1.join(timeout=86400)
            backend.close()

        except Exception as e:
            self.send_error(502, f'WebSocket proxy failed: {e}')

    def log_message(self, format, *args):
        # Only log errors, not every request
        if args and '50' in str(args[1:2]):
            sys.stderr.write(f"[{self.log_date_time_string()}] {format % args}\n")

if __name__ == '__main__':
    print(f"🌐 Paqtra SPA server on http://0.0.0.0:{PORT}")
    print(f"🔗 Proxying /api/* -> http://{API_HOST}:{API_PORT}")
    print(f"📂 Serving {DIRECTORY}")
    with http.server.ThreadingHTTPServer(('0.0.0.0', PORT), SPAProxyHandler) as httpd:
        httpd.serve_forever()
