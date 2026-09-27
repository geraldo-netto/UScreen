# T414: audio-gap follow-up

Read-only journal inspection found Bluetooth transport errors on September 27,
not a verified Blent audio defect. At 01:20:39 CEST PipeWire and WirePlumber report
a running Bluetooth output entering error and a transport failure. At 16:35:08
and 16:38:41 WirePlumber reports failure to release a transport whose Release
method no longer exists; later RFCOMM negotiation follows. These may describe
ordinary device disconnect/reconnect. No reported audible gap has been matched
to these timestamps, and no simultaneous Chrome playback trace exists.

The [retained excerpt](artifacts/2026-09-27-audio-followup/audio-journal.txt)
preserves timestamps and boot boundaries, replacing the Bluetooth identity and
hostname. [Versions](artifacts/2026-09-27-audio-followup/versions.txt) record
PipeWire 1.0.5, WirePlumber 0.4.17 and BlueZ 5.72 package revisions. Missing optional
libcamera/JACK services and HFP registration messages are not evidence of a cause
for A2DP playback gaps.

The earlier smooth sample and ERR=0 remain insufficient. Blent still transports
video only; Chrome audio goes through the Linux audio stack. The completed T597
change only reduces latency-summary computation, and makes no audio-fix claim.
T417's decoder diagnostics distinguish configuration/fallback when a matched
sample is eventually captured; they cannot measure Bluetooth audio.

A causal fix still requires a timestamped recurring gap with Chrome playback,
PipeWire/Bluetooth and host scheduling observations. The maintainer has been
asked whether the symptom currently recurs and what triggers it. If the symptom
is instead constant offset or drift, use the existing physical flash/click
procedure across sustained playback, seeks and recovery. No audio route, codec,
buffering preference, display attachment or Android lock state was changed.
T414 remains unresolved; no behavioral fix or artificial regression is claimed.
