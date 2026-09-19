# T492 evidence

See the [report](../2026-09-19-idle-cadence.md). `SHA256SUMS` covers this
directory; `evidence.tar.gz` has its own manifest for the extracted files.

| Archive directory | Contents |
| --- | --- |
| `usb` | Eight completed trials, sixteen encoder/decoder phases, plan, corpus identity, exported encoder policies, exact commands, encoded H.264, packet probes, callbacks and cleanup |
| `decoder-sources` | Exact copied Kotlin and original production sources plus measured APK hash |
| `harness` | Maintained collector, analyzer and relevant source snapshots |
| `validation` | Harness admission red/green evidence, Python suite, focused Rust/C and Android results, build log and complexity check |

No APK, build cache or raw NV12 corpus is included. Recreate the verified
`text.nv12` corpus with `scripts/benchmarks/codec-corpus.py`; its font/source
hashes and generation command are preserved in `usb/metadata.json`. Hardware
and library changes can change encoded output, so record new hashes on rerun.
The original encoded streams here allow independent keyframe verification.

With an available tablet and a freshly built separate decoder probe, the
collector is:

```sh
python3 scripts/benchmarks/profile-usb.py \
  --serial DEVICE_SERIAL --corpus /path/to/corpus \
  --plan /path/to/evidence/usb/plan.json \
  --provenance /path/to/decoder-probe/provenance.json \
  --output /tmp/uscreen-idle-check
python3 scripts/benchmarks/summarize-idle-cadence.py /tmp/uscreen-idle-check
```

For analysis only, run the second command on the extracted `evidence/usb`
folder. It uses stock `ffprobe` to match packet sizes/positions before labeling
independent pictures. `idle-summary.json` is the published copy of that output;
check for **eight trials / sixteen phases**, since the analyzer can also
summarize an incomplete run. Phase-level percentiles exclude the first second
of inputs, and the reported aggregate is the median of four phase percentiles.
Missing ACKs are retained rather than filled or mistaken for low latency.
`usb/join-check.json` and `validation/join-check.py.txt` preserve independent
IDR/SPS/PPS checks and standalone software decoding of all distinct key-packet
payloads. This supplements keyframe timing; it is not a production reconnect
measurement or an Android optical-presentation test.

`usb/completion.json` records probe removal, production foreground, remaining
ADB routes and unchanged host process identities. The collector uses isolated
sockets and stock encoders; it does not attach an EVDI display or exercise
production late-client admission/backlog recovery.
