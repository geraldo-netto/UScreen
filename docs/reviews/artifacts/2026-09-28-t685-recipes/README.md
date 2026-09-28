# T685 encoder recipe evidence

Portable tests pass; all eight recipe functions exceed 80% executable-line
coverage individually on both Linux and native Windows. Native suite: 144 tests
pass as ordinary user. Complexity: 6,610 functions, none above 9.

Native FFmpeg 8.1.2 accepted the recipe syntax. libx264 encoded four generated
160x120 frames (10,632 bytes). NVENC failed for missing nvcuda.dll; AMF failed for
missing amfrt64.dll; QSV could not create an MFX session (-9). No GPU capability
or performance claim follows. T677 retains GPU/tablet validation and live admission.

Replay decompressed counters with scripts/coverage/report.py check using manifest,
scope common/src/encoding/windows, corresponding --rust-platform and, for Windows,
native sources.json and prefix C:\src\blent. Design/tradeoffs are recorded in
../../2026-09-28-windows-encoder-recipes.md. Tests remain in the normal suite.
