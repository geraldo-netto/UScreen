# T530 encoder probe evidence

Fresh Linux and ordinary-user Windows LLVM collections pass all 67 scoped
production functions individually at >=80% executable-line coverage. Linux passes
246 host and 146 common tests; Windows passes 135 host and 144 common tests.
Two pre-existing opt-in benchmark tests remain ignored on both platforms; no
regression was skipped. All 41 existing Linux selection-worker tests also pass.
Complexity: 6,635 functions, none above 9. Windows GNU cross-check passes.

Native tests inspect 65 real libx264 packets, effective worker SEI and matching
tablet profiles. Automatic/explicit selection, unavailable encoders, absent
programs, failed inspection, incompatible profiles, malformed/oversized inventory,
packet/format bounds and stale identities/settings are retained in the suite.
Existing real H.264/HEVC packetizer decode regressions now run natively on Windows.
The shared benchmark clock uses native thread CPU time on each platform; no new
performance claim or benchmark acceptance is made.

The first native attempt lacked ffprobe and correctly failed. Installing the
matching retained FFmpeg 8.1.2 ffprobe made the unchanged probe tests pass.
ffprobe.exe SHA256: b49ccc7c6547b141ad5a2f6ec69cc04323d7133d7704d70b331b904c63eecb07.
GPU runtime/physical tablet acceptance remains T677/T522.

Replay decompressed evidence using scripts/coverage/report.py check, the retained
manifest and matching --rust-platform. Scopes: common/src/encoding.rs,
host/src/encoder_probe, host/src/annex_scan.rs, host/src/annex_b.rs,
host/src/framed_annex_b.rs and host/src/capture/probe_format.rs. Windows also
requires sources.json and prefix C:\src\blent. Design and tooling invocation:
../../2026-09-28-windows-encoder-probes.md.
