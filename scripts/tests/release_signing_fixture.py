"""T505: deterministic SDK reports for offline packaging fixtures, not real APKs."""
from test_release_apk import MANIFEST, SIGNER


def install_tools(directory):
    directory.mkdir(parents=True, exist_ok=True)
    for name, report in {'apksigner': SIGNER, 'aapt2': MANIFEST}.items():
        tool = directory / name
        tool.write_text("#!/bin/sh\ncat <<'REPORT'\n" + report + "\nREPORT\n")
        tool.chmod(0o755)
