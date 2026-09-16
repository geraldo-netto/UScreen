"""Loaded as sitecustomize by release tests; all urllib HTTP stays offline."""
import hashlib
import io
import json
import os
from pathlib import Path
import urllib.error
import urllib.parse
import urllib.request

root = Path(os.environ['USCREEN_TEST_ROOT'])


def fake_urlopen(request, *args, **kwargs):
    authorized = request.get_header('Authorization') == 'Bearer ' + os.environ['GH_TOKEN']
    method = request.get_method()
    with (root / 'requests').open('a') as log:
        log.write(json.dumps({'url': request.full_url, 'method': method, 'authorized': authorized}) + '\n')
    path = root / 'api-state'
    state = json.loads(path.read_text()) if path.exists() else {'assets': [], 'draft': True, 'published': False}
    data = request.data
    if '/assets?name=' in request.full_url:
        index = len(state['assets'])
        failure = os.environ.get('USCREEN_TEST_FAILURE')
        if os.environ.get('USCREEN_TEST_FAIL_INDEX') == str(index):
            if failure == 'http':
                raise urllib.error.HTTPError(request.full_url, 422, 'upload failed', {}, None)
            if failure == 'api':
                return io.BytesIO(b'{"message":"upload failed"}')
        payload = data.read() if hasattr(data, 'read') else data
        name = urllib.parse.parse_qs(urllib.parse.urlsplit(request.full_url).query)['name'][0]
        result = {'id': index + 1, 'name': name, 'state': 'uploaded', 'size': len(payload),
                  'digest': 'sha256:' + hashlib.sha256(payload).hexdigest()}
        if failure == 'digest' and index == 0:
            result['digest'] = 'sha256:' + '0' * 64
        state['assets'].append(result)
    elif method == 'POST':
        body = json.loads(data)
        state['draft'] = body.get('draft', False)
        state['published'] = not state['draft']
        result = {'id': 123, 'draft': state['draft'], 'tag_name': 'v1.2.3'}
    elif method == 'PATCH':
        body = json.loads(data)
        state['draft'] = body['draft']
        state['published'] = not state['draft']
        result = {'id': 123, 'draft': state['draft'], 'tag_name': 'v1.2.3'}
    elif method == 'GET' and '/assets' in request.full_url:
        result = state['assets']
        if os.environ.get('USCREEN_TEST_FAILURE') == 'missing':
            result = result[:-1]
    else:
        raise AssertionError('Unexpected HTTP request: ' + request.full_url)
    path.write_text(json.dumps(state))
    return io.BytesIO(json.dumps(result).encode())


urllib.request.urlopen = fake_urlopen
