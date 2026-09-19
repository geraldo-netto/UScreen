"""T497 subprocess hook; measurement errors must not silently lose coverage."""
import os
import sys


def start():
    if not os.environ.get('USCREEN_PYTHON_CALLS'):
        return
    try:
        import coverage
        from calls import start as observe_calls
        coverage.process_startup()
        observe_calls()
    except Exception as error:
        print(f'UScreen coverage startup failed: {error}', file=sys.stderr, flush=True)
        os._exit(86)
