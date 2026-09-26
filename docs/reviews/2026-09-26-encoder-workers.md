# T612: bounded encoder worker selection

The existing CLI auto selector compares software H.264 worker budgets 1, 2 and 4
before other encoders. One worker remains the fallback. `encoder_workers=0`
means Auto; saved/manual and CLI values 1–128 override that dimension. The
in-process adapter shares manual policy and retains one worker for Auto.
No runtime or conversion-pool default changed, and no dependency was added.

Candidates retain requested workers and the effective x264 count when the probe
SEI supplies it; missing effective counts remain unknown. Worker-only changes
restart the encoder, not the helper. Render evidence is worker/generation-bound.
The existing 30-second host-probe and 36-second live budgets, cancellation,
geometry/peer ownership and opt-in historical cache remain. Cache schema/context
now include the worker budget and resource observations.

Shared policy compares CPU/frame and sampled peak RSS through an explicit Linux
process-counter adapter. A replacement must satisfy existing delivery,
first-frame quality, p95/p99, rate and startup guards, plus ≤10% extra encoder
CPU/frame and ≤20% extra sampled RSS. Unknown counters do not certify a better
candidate. Quality references the lowest requested software budget regardless
of host probe ranking; the permanent T612 ordering regression failed before
that correction and passes afterward. Failed one-worker live measurement cannot
certify a higher automatic budget. These tolerances are policy, not measured
performance improvements or total-machine resource limits.

Validation: 52 selector tests pass, including worker identity, manual limits,
cache context, cancellation/reconnect/resize, quality-reference ordering,
resource rejection and fallback. Full default workspace tests pass; all-feature
library/application tests pass. Combined Linux coverage meets 80% for all
1,252 maintained Rust functions; no function exceeds complexity nine. The two
files revised after the full run were recollected in full, and their old counters
were excluded. Source-hash attestation accompanies the report. The report retains
61 unavailable non-Linux functions; existing platform TODOs still apply.
Temporary FFmpeg development headers and RPM tools supplied missing local test
prerequisites; no system package installation was needed. T626 separately
corrected the pre-existing CLI fixture's stale worker expectation.

Native 1280×800 calibration exercised the bounded trials and returned to the
one-worker libx264 fallback. The first trial began before capture setup finished;
later named-decoder trials produced encoded packets without accepted receipts.
Fallback epoch 4 immediately resumed render ACKs. Consequently **no native
higher-worker winner or speedup is claimed**. T627 retains investigation and
missing native winner evidence. The fallback stayed active for T553's separately
approved combined display/camera sample. Manual override and both encoder
adapters are covered automatically; this native run does not establish every
worker count or hardware encoder as supported.

[Evidence](artifacts/2026-09-26-task-batch/t612/).
User-facing configuration and measurement limits are in [video codecs](../video-codecs.md#software-encoder-worker-capacity).
