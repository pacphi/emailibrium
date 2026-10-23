#!/usr/bin/env python3
"""Probe a built backend with an empty DB and mock embeddings; no live mail.

Build with --features test-vectors and provide --binary and --output-dir.
The process and temporary database are always cleaned up.
"""

import argparse
import hashlib
import http.cookiejar
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', required=True, type=Path)
parser.add_argument('--output-dir', required=True, type=Path)
options = parser.parse_args()
evidence = options.output_dir.resolve()
evidence.mkdir(parents=True, exist_ok=True)
result_path = evidence / 'native-http-smoke.json'
result_path.write_text(json.dumps({'status': 'not_completed'}) + '\n')
if not __debug__:
    parser.error('Run without Python optimization; this check requires assertions')
binary = options.binary.resolve()
if not binary.is_file():
    parser.error('The backend binary does not exist')
with binary.open('rb') as binary_file:
    binary_sha256 = hashlib.file_digest(binary_file, 'sha256').hexdigest()

class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None

with tempfile.TemporaryDirectory(prefix='emailibrium-http-smoke-') as temp:
    runtime = Path(temp) / 'backend'
    runtime.mkdir()
    (Path(temp) / 'config').mkdir()
    with socket.socket() as probe:
        probe.bind(('127.0.0.1', 0))
        port = probe.getsockname()[1]
    root_url = f'http://127.0.0.1:{port}'
    token = secrets.token_hex(32)
    key = secrets.token_hex(32)
    (runtime / 'config.yaml').write_text(f'''
host: "127.0.0.1"
port: {port}
database_url: "sqlite:{runtime}/mail.db?mode=rwc"
store:
  backend: "memory"
  path: "{runtime}/vectors"
embedding:
  provider: "mock"
  dimensions: 384
generative:
  provider: "none"
redis:
  enabled: false
backup:
  enabled: false
''')
    env = {k: v for k, v in os.environ.items()
           if not k.startswith('EMAILIBRIUM_') and k not in {
               'JWT_SECRET', 'OAUTH_ENCRYPTION_KEY', 'OPENAI_API_KEY',
               'ANTHROPIC_API_KEY', 'OPENROUTER_API_KEY', 'COHERE_API_KEY',
               'GOOGLE_API_KEY'}}
    env.update(JWT_SECRET=token, OAUTH_ENCRYPTION_KEY=key, APP_ENV='development', RUST_LOG='info')
    log_path = evidence / 'native-http-smoke.log'
    checks = {}
    plain = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
    cookies = http.cookiejar.CookieJar()
    browser = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect(), urllib.request.HTTPCookieProcessor(cookies))

    def request(path, method='GET', headers=None, opener=plain, payload=None):
        data = json.dumps(payload).encode() if payload is not None else None
        req = urllib.request.Request(root_url + path, data=data, method=method, headers=headers or {})
        try:
            with opener.open(req, timeout=5) as response:
                return response.status, response.read(), response.headers
        except urllib.error.HTTPError as error:
            return error.code, error.read(), error.headers

    with log_path.open('w') as log:
        proc = subprocess.Popen([str(binary)], cwd=runtime, env=env, stdout=log, stderr=subprocess.STDOUT)
        try:
            deadline = time.monotonic() + 30
            while True:
                if proc.poll() is not None:
                    raise RuntimeError('Backend exited before readiness; inspect native-http-smoke.log')
                try:
                    status, body, _ = request('/healthz')
                    if status == 200:
                        assert json.loads(body) == {'status': 'ok'}
                        checks['minimal_public_health'] = True
                        break
                except (OSError, urllib.error.URLError):
                    pass
                if time.monotonic() >= deadline:
                    raise RuntimeError('Backend did not become ready')
                time.sleep(0.1)
            for path in ['/api/v1/auth/accounts', '/api/v1/vectors/health', '/api/v1/mcp']:
                assert request(path)[0] == 401, path
            checks['unauthenticated_rest_and_mcp_denied'] = True
            assert request('/api/v1/auth/accounts', headers={'Authorization': 'Bearer wrong'})[0] == 401
            checks['wrong_credential_denied'] = True
            auth = {'Authorization': 'Bearer ' + token}
            assert request('/api/v1/auth/accounts', headers=auth)[0] == 200
            checks['authenticated_actual_rest'] = True
            assert request('/api/v1/mcp', headers={**auth, 'Origin': 'https://untrusted.example'})[0] == 403
            checks['mcp_origin_boundary'] = True
            rpc_headers = {'Content-Type': 'application/json', 'Accept': 'application/json, text/event-stream'}
            initialize = {'jsonrpc': '2.0', 'id': 1, 'method': 'initialize', 'params': {
                'protocolVersion': '2025-03-26', 'capabilities': {},
                'clientInfo': {'name': 'native-http-smoke', 'version': '1'}}}
            assert request('/api/v1/mcp', 'POST', rpc_headers, payload=initialize)[0] == 401
            assert request('/api/v1/mcp', 'POST', {**rpc_headers, **auth, 'Origin': 'https://untrusted.example'}, payload=initialize)[0] == 403
            rpc_headers.update(auth)
            status, body, headers = request('/api/v1/mcp', 'POST', rpc_headers, payload=initialize)
            assert status == 200
            session_id = headers.get('mcp-session-id')
            assert session_id, 'MCP initialize must issue a session'
            rpc_headers['mcp-session-id'] = session_id
            assert request('/api/v1/mcp', 'POST', rpc_headers, payload={
                'jsonrpc': '2.0', 'method': 'notifications/initialized'})[0] == 202
            status, body, _ = request('/api/v1/mcp', 'POST', rpc_headers, payload={
                'jsonrpc': '2.0', 'id': 2, 'method': 'tools/list', 'params': {}})
            assert status == 200
            frames = [json.loads(line[5:].strip()) for line in body.decode().splitlines()
                      if line.startswith('data:') and line[5:].strip()]
            assert len(frames) == 1 and frames[0]['id'] == 2
            assert isinstance(frames[0]['result']['tools'], list) and frames[0]['result']['tools']
            checks['authenticated_actual_mcp_handshake_and_discovery'] = True
            status, _, headers = request('/api/v1/session', 'POST', auth, browser)
            assert status == 200
            cookie = headers.get('Set-Cookie', '')
            assert 'HttpOnly' in cookie and 'SameSite=Strict' in cookie
            assert request('/api/v1/auth/accounts', opener=browser)[0] == 200
            checks['browser_cookie_session'] = True
            assert request('/api/v1/session', 'DELETE', opener=browser)[0] == 200
            assert request('/api/v1/auth/accounts', opener=browser)[0] == 401
            checks['logout_revokes_access'] = True
            status, body, _ = request('/api/v1/auth/callback?%63ode=encoded-code-canary&%73tate=encoded-state-canary')
            assert status == 400 and b'OAuth request expired or invalid' in body
            status, body, _ = request('/api/v1/auth/callback?code=synthetic-code&state=gmail%3Asynthetic-unissued-state')
            assert status == 400 and b'OAuth request expired or invalid' in body
            checks['unissued_callback_rejected'] = True
        finally:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()
    logged = log_path.read_text()
    for secret in [token, key, 'encoded-code-canary', 'encoded-state-canary',
                   'synthetic-code', 'synthetic-unissued-state']:
        assert secret not in logged, 'Sensitive value leaked into process logs'
    checks['no_credentials_or_callback_canaries_in_logs'] = True
    result = {'status': 'passed', 'checks': checks, 'binary': str(binary),
              'binary_sha256': binary_sha256,
              'limits': 'Actual native backend, empty disposable SQLite database, mock embeddings, no model inference or live provider connection.'}
    result_path.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result))
