# Passive YouTube observation — 2026-09-20

T414 remains open: the maintainer reports intermittent **audio gaps**, with
Linux Chrome video displayed on the tablet and Bluetooth headphones paired to
Linux. Playback was smooth during collection. T553's combined display/camera
transport comparison also remains open; camera sharing was inactive here.

UScreen carries video only. PipeWire routed Chrome directly to the BW01
Bluetooth A2DP/SBC sink. The short [`pw-top` sample](pw-top.log) showed ERR=0
for both Chrome and the Bluetooth sink. No audio route, codec, buffering or
UScreen settings were changed. This smooth sample does not identify or rule
out the cause of intermittent gaps, including brief scheduling or radio stalls.
The sink's address is replaced by its descriptive name in the retained log.

The running display used 1280×800 at 30 FPS, hardware H.264 Constrained
Baseline, CQP 18 on renderD128 (the discrete AMD GPU). A saved bitrate value is
not measured traffic or an enforced CQP bitrate ceiling. Running executable
hashes and selected settings are retained in [`metadata.json`](metadata.json).

[`host-sample.json`](host-sample.json) covers 15.018 seconds. EVDI helper CPU
averaged 5.59% of one logical core, FFmpeg 2.26%, and the busiest sampled Chrome
process 7.92%. GPU readings are four isolated observations: renderD128/card3
was 0/0/0/2%; integrated card4 was 99/0/0/45%. Neither averages nor these sparse
peaks establish the cause of a brief glitch or sustained GPU saturation.

[`transport-sample.json`](transport-sample.json) covers 10.044 seconds. The
localhost display socket into ADB advanced 16,695,347 acknowledged bytes,
about **13.30 Mb/s**, with zero send queue at the two endpoints. This is encoded
display payload into ADB, not physical USB throughput, ADB internal backlog or
end-to-end latency. It is not a simultaneous camera/display load measurement.

Next useful evidence is a correlated sample while the audio gaps occur:
PipeWire/Bluetooth errors, Chrome playback state and host scheduling. Keep
Bluetooth audio gaps separate from constant A/V offset or growing drift. If
those also recur, T414 retains the physical flash/click and shared-clock
measurement requirements. T554's changed-region conversion work makes no audio
fix claim. Do not reset ADB, lock Android or detach the live EVDI display to
collect this evidence.
