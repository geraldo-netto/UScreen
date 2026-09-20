# T563: pinned FFmpeg 6.1.6 in the Linux AppImage

The AppImage now builds and uses its own FFmpeg/ffprobe 6.1.6. The installed
image is ready for the next normal UScreen start. The existing desktop/tablet
session still uses its extracted FFmpeg 5.1.9; no live EVDI restart was attempted.
This is a packaging and compatibility result, not a measured speed improvement.

## Build and codec policy

[Upstream's download page](https://ffmpeg.org/download.html) listed 6.1.6,
released 2026-06-20, as the latest 6.x release when checked on 2026-09-20 UTC.
The source archive's signature verified against the FFmpeg release key
`FCF986EA15E6E293A5644F10B4322F04D67658D8`. The permanent recipe pins its
SHA-256 and the NV codec headers' exact commit in
[`ffmpeg.json`](../../../packaging/appimage/ffmpeg.json).

The Debian 12 build uses unmodified upstream source, static internal libav
libraries, generic x86-64 with runtime CPU dispatch, and private bundled x264,
x265, libvpx and libaom libraries. VAAPI/NVENC interfaces are enabled; vendor GPU
drivers and the matching glibc/loader remain host dependencies. The existing
low-latency runtime options and user-controlled capacity settings are preserved.
The Linux packaging adapter does not change Android, shared configuration,
codec negotiation, Windows/macOS interfaces, or the optional in-process encoder.

Automatic selection already filters for tablet compatibility, measured host
capacity and quality, then compares a bounded shortlist using session
packet-ready-to-render-ACK timing and supervises render progress. This is not
an exhaustive fastest-codec guarantee. Newer codecs and compiled hardware
backends alone establish neither availability nor better performance. The
current manual `h264_vaapi_baseline` choice and **30 FPS** were left intact.

## Validation

Permanent T563 regressions were added before packaging changes. The original
wrapper loaded the conflicting host codec fixture (`81`) instead of the private
one (`31`); package selection also failed the new pinned-prefix contract.
Both pass after the changes. Tests retain malformed input, checksum, archive
traversal/link/device, extraction-size, build-job, source-retention, executable
identity, required-codec and private-library boundary coverage.

The first candidate omitted libx265. Existing permanent T448 HEVC framing
coverage failed with `Unrecognized option 'x265-params'`. Restoring libx265
made the same test pass; it remains outside automatic encoder selection.

| Check | Result |
| --- | --- |
| Normal Python script suite | 265 passed, including 10 T563 and 25 AppImage tests |
| Host tooling suite used by script coverage | 35 passed |
| Essential-script executable-line coverage | All 61 Python and 57 shell functions meet 80%; all 13 new FFmpeg functions reach 100% |
| Complexity | 5,141 functions checked; none above cyclomatic complexity 9 |
| Final bundled FFmpeg: framing | 10 passed; one pre-existing opt-in benchmark ignored |
| Final bundled FFmpeg: camera conversion/preview | 12 passed |
| Final bundled FFmpeg: T116 idle join-point regression | Passed at its 60/90 FPS targets and current five-update/s idle cadence |
| Clean pinned Debian 12 container | Dependency closure, extraction, bundled encoding, registration, idle daemon, GUI and independent runtime lifetime passed |

The native codec probe used the existing shared encoder policy, 30 FPS,
20,000 kbps configured bitrate, QP/CRF 18, and twelve synthetic 320×192 frames.
Successful streams all decoded to twelve NV12 frames. These tiny, single-run
trials test availability and compatibility; their elapsed times are not codec
rankings. VAAPI CQP does not enforce the configured bitrate ceiling.

| Native encoder/profile | Result on this Linux host |
| --- | --- |
| H.264 VAAPI High and Constrained Baseline | Both encoded and decoded |
| HEVC VAAPI | Encoded and decoded |
| libx264, libvpx-vp9, libaom-av1 | All encoded and decoded |
| H.264/HEVC/AV1 NVENC | Unavailable: host cannot load `libcuda.so.1` |
| VP9/AV1 VAAPI | Unavailable: GPU/driver exposes no usable encoding entrypoint |

An earlier 6.1.6 candidate, before restoring libx265, also completed the isolated
12-trial H.264 VAAPI writer matrix: 519 frames and 84 independent join points
decoded. Final-build evidence above includes the existing idle and framing
regressions; the earlier matrix does not establish live Android performance.
T564 retains unavailable hardware validation; T565 retains live-session
validation after a safe normal startup. No broad hardware campaign was reopened.

## Installed artifact and activation boundary

The stable image was atomically replaced, with the prior image retained as
`~/.local/share/uscreen/appimage/UScreen-before-T563-17d656cdd66d.AppImage`.
The matching corresponding-source archive is alongside the new image. The
Xorg, daemon, EVDI helper and live FFmpeg process identities stayed unchanged.
Killing the existing encoder would enter the crash path that terminates its
helper (`SessionChanges::keep_helper`); it is not a safe hot-upgrade mechanism
while T222 remains unresolved. No Android restart, lock or settings change was
performed. The new image selects bundled FFmpeg automatically on normal start.

| Artifact | SHA-256 |
| --- | --- |
| Installed `UScreen.AppImage` | `0d3c2984770ee810a33fee1d8d605fb413915c3ea937991753e5e41fc4f5c4d3` |
| `uscreen-1.2.3-AppImage-sources.tar.gz` | `7a98be517b3c26e80c7c9f2adcb5c174d44280f9cc94df40f91fdce146d4a12f` |
| Bundled FFmpeg executable | `514afb99d317124690a90e60d3a645d5d90266125a426ee136bdfc90c81bcba4` |
| Bundled ffprobe executable | `dea0579c78144e8638a84b5a77aa4fcb4a466e99ae8ba96cce78fe6e005f80e2` |

The source asset includes exact dependency sources, licenses and FFmpeg build
inputs/manifest. These are local artifacts, not a published release. GitHub
workflows remain manual-only and were not run. Their build container now has
the dependencies required for the pinned FFmpeg recipe.

[`evidence.tar.gz`](evidence.tar.gz) retains red/green logs, final native probe
commands/results, build identity, script coverage and its source-hash manifest,
complexity, clean-container validation and the installation record. Raw local
build caches and binaries are not committed. The package was assembled before
this final validation report was written; the report's artifact hashes identify
the tested image exactly.
