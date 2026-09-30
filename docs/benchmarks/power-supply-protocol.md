# Alternative USB power supply comparison protocol

T683 prepares a comparison of two identified power arrangements while Blent keeps
the same USB display transport. Preparation is complete; T739 retains execution.
The [T538 spot checks](2026-09-20-usb-hub-power/README.md) observed uncontrolled
cable changes and near-full battery levels, so they cannot establish which supply
maintains data or provides adequate sustained charging.

## Eligibility and prerequisites

Complete the matched T388 USB comparison before measuring supply differences.
For both arrangements, record charger/hub models, rated input/output capabilities,
the exact upstream/downstream/charging ports, cable identities and wiring. Include
a port-to-port diagram or equivalent table. Name the tablet data role and host
USB topology as observed; do not infer either from a connector or marketing label.
Do not improvise power injection or change wiring during an accepted phase.

An eligible arrangement must preserve authorized ADB, the selected tablet identity,
USB display delivery and user input while providing power. T538 must first verify
that the proposed hub/charger/cables support those functions together. If an
arrangement requires network transport, it belongs in T738's network experiment
and is not an isolated USB supply comparison. No extra tablet is required.

Reserve an undisturbed roughly 90-minute host/tablet window after both arrangements
pass a short preflight. Begin below full charge, within a comparable documented
battery/temperature range; agree that range before the run. A trial at reported
100% cannot establish the available sustained charging rate at lower charge.

## Matched trials

Reuse the configuration manifest, observation boundaries, foreground/lock guards,
rejection rules and restoration procedure from the
[network protocol's shared method](network-power-protocol.md#shared-trial-method).
Here the active transport remains known USB and the Battery saver selection stays
fixed at the matched T388 setting. Hold host/APK versions, encoder/GPU, resolution,
refresh, quality, brightness, workload, audio/camera state and observer schedule
constant. Supply arrangement is the only intentional change.

For static and motion workloads, run two balanced repetitions of arrangements A
and B, reversing order in the second repetition. Use 30-second warm-ups and
five-minute measurement windows: eight active trials take about 44 minutes.
Add a ten-minute streaming-off control for each arrangement, keeping the same
screen/brightness state. Allow setup and stabilization time within the reserved
window; stop rather than silently extending it or accepting mismatched phases.

Make wiring changes only between phases. Record disconnect/reconnect timestamps
and recovery, then verify USB identity, display/input delivery, selected settings
and actual CPU/Wi-Fi locks before starting the next warm-up. A route change, lost
data connection, unexpected default-device change, focus intrusion or missing
observation rejects the phase. Preserve raw evidence and restore the original
wiring/preferences with the maintainer when finished.

## Evidence and decision

Record signed net battery current, integrated net charge, gauge readings and
resolution, battery voltage/temperature, charging status, ADB/USB topology,
application/session identities, frame cadence and software latency. Existing
`rect-power-report.py` can analyze a compatible battery Perfetto trace, but
`rect-power.py` runs a different local-replay experiment and is not this plan's
streaming orchestrator. Keep raw traces and exact collector versions.

Separate advertised charger/USB maxima, charging flags, net battery flow and
measured electrical input. Input watts require an identified calibrated meter
that preserves the data/power arrangement; they cannot be calculated from USB's
advertised maximum alone. A coarse unchanged gauge or short near-neutral current
sample is inconclusive, not proof of adequate supply.

Accept data continuity only with sustained display/input evidence. Describe
charging adequacy only for the measured workload, charge range and duration, with
paired uncertainty and any thermal effects. Retain unsuitable arrangements and
rejected trials explicitly. This protocol does not reopen the declined broad
hardware campaign; a software fault discovered during execution needs its own
permanent failing automated regression before a fix.
