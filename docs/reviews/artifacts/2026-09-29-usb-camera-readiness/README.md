T540 physical USB acceptance passed September 29, 2026, 10:56–10:58 CEST.

- `accept.py`: bounded operator-assisted harness, executed successfully (exit 0).
- `acceptance.json`: original numeric camera checks and wall-clock transition times.
- `host-events.log`: ANSI-stripped timestamped host journal records for the test window;
  untimestamped FFmpeg/helper progress lines omitted. Log timestamps are UTC.
- `observation.json`: operator confirmation, post-test UI labels and timing boundary.
- `SHA256SUMS`: hashes of all other files in this directory.

Uses the real camera page from sibling `2026-09-29-camera-acceptance/index.html`.
Installed source/build identities and automated coverage remain in sibling
`2026-09-29-display-acceptance/deployment.json` and its validation evidence.
Camera settings: front, 1280x720, 30 FPS; inactive rear also opened by real Chrome.
Only numeric canvas summaries retained, no camera images or audio.

Run with the isolated Chrome/page already listening on localhost 19221/19222:
`/backups/disk2/blent-work/t715/tools/bin/python -u docs/reviews/artifacts/2026-09-29-usb-camera-readiness/accept.py`
The harness uses this installation's tablet serial and writes to
`/backups/disk2/blent-work/t540`; arrange a cable operator before running.
The passing run observed physical removal/reconnection, not an ADB restart.
See the parent review for display recovery, timing limits and remaining T558 work.
