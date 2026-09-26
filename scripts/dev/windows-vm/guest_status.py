"""Read a bounded status file from the dedicated Windows development guest."""
import base64
import time

MAX_BYTES = 1024 * 1024


def read_command(path):
    if not isinstance(path, str) or not path or '\0' in path:
        raise ValueError('Expected a nonempty guest file path without NUL')
    encoded_path = base64.b64encode(path.encode('utf-16le')).decode('ascii')
    script = r'''
$ErrorActionPreference='Stop'
$path=[Text.Encoding]::Unicode.GetString([Convert]::FromBase64String('PATH_BASE64'))
$share=[IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete
$stream=[IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::Read,$share)
try {
    if ($stream.Length -gt 1048576) { throw 'Guest status exceeds 1 MiB' }
    $reader=[IO.StreamReader]::new($stream,[Text.Encoding]::UTF8,$true)
    $value=$reader.ReadToEnd()
    if ([Text.Encoding]::UTF8.GetByteCount($value) -gt 1048576) { throw 'Guest status exceeds 1 MiB' }
    [Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
    [Console]::Write($value)
} finally { $stream.Dispose() }
'''.replace('PATH_BASE64', encoded_path)
    return base64.b64encode(script.encode('utf-16le')).decode('ascii')


def decode_result(result):
    if result.get('out-truncated') or result.get('err-truncated'):
        raise RuntimeError('Guest status output was truncated')
    if result.get('exitcode') != 0:
        raise RuntimeError('Guest status read failed')
    output = base64.b64decode(result.get('out-data', ''), validate=True)
    if len(output) > MAX_BYTES:
        raise ValueError('Guest status exceeds 1 MiB')
    return output.decode('utf-8-sig')


def read_file(request, path, timeout=15):
    """Use a process-owned read: a lost host reply cannot orphan a QGA handle."""
    command = read_command(path)
    if not 0 < timeout <= 60:
        raise ValueError('Status timeout must be between zero and 60 seconds')
    process = request('guest-exec', {
        'path': 'powershell.exe',
        'arg': ['-NoProfile', '-NonInteractive', '-EncodedCommand', command],
        'capture-output': True,
    })
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = request('guest-exec-status', {'pid': process['pid']})
        if result.get('exited'):
            return decode_result(result)
        time.sleep(.1)
    # Do not retry launch: the accepted guest process closes its own file even
    # if the host client disappears or never receives its completion response.
    raise TimeoutError('Guest status read did not finish before its deadline')
