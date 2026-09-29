# T718: Linux/Android microphone integration

Microphone capture, authenticated ADB transport, a selectable PipeWire **Blent
Microphone** source, explicit CLI/GUI Start/Stop, Android consent/settings and
background-service ownership are implemented. [Current behavior](../audio.md)
and the [accepted contract](2026-09-29-audio-input.md) distinguish this path from
speaker and duplex work under T719/T720.

[Retained evidence](artifacts/2026-09-29-audio-microphone/README.md) includes the
permanent regressions and red/green results, native PipeWire and desktop-consumer
checks, source-matched per-function counters, complexity results and signed APK
identity. All 201 scoped Rust functions, 84 Kotlin methods and 127 essential-script
functions meet the 80% executable-line threshold. No new coverage exclusions were
introduced. Audio remains unsupported on PulseAudio-only hosts, Windows and macOS.

The connected tablet delivered live PCM to an ordinary desktop audio consumer;
Stop, foreground loss and OS silencing removed the source. Existing audio defaults
stayed unchanged. No audio recording was saved. The measured source startup and
retirement timings do not establish acoustic delay. The live signal was quiet;
intentional speech/intelligibility is not yet confirmed. Physical route/USB
changes and duplex AEC/drift/latency acceptance also remain outstanding.

The implementation does not continuously correct native device-clock drift yet;
T720 must connect actual device counters to the existing shared correction policy.
The current queue still bounds backlog, expires old PCM and emits silence on
underflow. AEC-enabled status is capability evidence, not acoustic effectiveness.
