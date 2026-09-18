# Live profile change and saved tablet setup — T429

On 2026-09-18 at 20:58:37 CEST, an authenticated control request changed the
installed host from `h264_vaapi` to `h264_vaapi_baseline`. The daemon saved the
choice and playback resumed using Constrained Baseline/CAVLC. The transition
also exposed an unexpected missing-FIFO failure and helper reattachment. No
behavioral fix was made during that configuration task. Subsequent
[T429 regression work](2026-09-18-fifo-ownership.md) reproduced unowned-manager
cleanup deleting the FIFO and fixed its ownership. This historical trace alone
does not identify which process deleted the path.

## Saved configuration

The user requested a setup suited to saving this tablet's battery. These are
personal saved settings, not changes to project-wide defaults:

| Setting | Result |
| --- | --- |
| Encoder | Changed to hardware VAAPI H.264 Constrained Baseline/CAVLC |
| Android Battery saver | Already enabled; retained |
| Android extra CPU/Wi-Fi locks | Neither held during the observed USB stream |
| App brightness / display refresh | Retained 50% / 60 Hz |
| Stream | Retained 1280×800, 60 FPS target, quality 18, scale 1, eight-bit |
| Bitrate preference | Retained 20,000 kbps; VAAPI CQP does not enforce this ceiling |
| Capture pipe | Retained 4 MiB; effective capacity confirmed by the helper |
| Statistics overlay | Retained off |

The [profile measurements](../benchmarks/2026-09-18-profile-selection.md) support
Constrained Baseline for latency and image quality on this hardware. They do not
establish an energy winner: its motion stream also used more bytes than High
profile. Battery saver removes extra locks while preserving display/stream
settings; the [sustained power comparison](../benchmarks/2026-09-18-power-validation.md)
remains incomplete. No battery-saving percentage or freedom from USB-powered
discharge is claimed. The optional 30 FPS tradeoff was left pending the user's
preference; no frame-rate change was made in this observation.

## Installed software and transition

The [configuration and process snapshot](2026-09-18-low-latency-live/setup.json)
records executable/APK hashes. They match the earlier
[deployment record](../benchmarks/2026-09-18-power-validation/deployment.json):
host `52fcd75`, Android runtime sources `bdaa17a`. Checkout `9c917bb` was **not**
deployed. The FIFO writer/recovery implementation in these installed sources is
unchanged in that checkout; later capture changes add decoder selection and
conversion-pool configuration.

The request used the existing authenticated WebSocket `config` message with
only `encoder` supplied. It briefly replaced the tablet's control connection;
the tablet subsequently reconnected. It did not request a service restart,
geometry/FPS change or module reload. This differs from the
[Linux GUI save path](2026-09-18-low-latency-transition.md), which requests a
full daemon restart.

The [filtered host journal](2026-09-18-low-latency-live/host-transition.txt)
records this sequence (timestamps below are UTC):

| Time | Event |
| --- | --- |
| 18:58:37.996 | Host accepts the Constrained Baseline setting |
| 18:58:38.003 | New encoder preference persisted |
| Before 18:58:38.086 | Helper reports an incomplete frame and retires its FIFO writer |
| 18:58:38.086–38.207 | Replacement FFmpeg cannot open `capture.fifo`: ENOENT; supervisor reports encoder failure |
| 18:58:38.223 | Existing helper exits after recovery tears it down |
| 18:58:48.407 / 58.926 | Video client reconnects |
| 18:58:53.411 | Codec configuration still unavailable after five seconds |
| 18:58:59.329 | Replacement helper connects to the existing EVDI device |
| 18:58:59.641 | Constrained Baseline codec configuration extracted and sent |
| 18:59:04.659 | First logged resumed latency window has p50 18.6 ms, p95 22.0 ms |

Acceptance to codec-configuration delivery was about 21.6 seconds. This is not
an optical interruption measurement or an exact first-render timestamp. The
first resumed window's 21,775.7 ms maximum includes transition history and must
not be presented as steady-state latency.

Host daemon PID 149897, Cinnamon PID 1897918 and Xorg PID 1897166 stayed the same.
The helper changed from PID 149961 to 651015, and FFmpeg from 150084 to 651155.
The observed FFmpeg command and stream header confirm `constrained_baseline`,
`cavlc`, zero B-frames and 1280×800 at a 60 FPS target. Streaming remained active
in subsequent observations, with logged window medians around 19–20 ms. The
preceding High-profile windows were around 180–203 ms on the current sparse
content. This uncontrolled before/after check confirms resumed playback, not a
new controlled performance or power benchmark. Historical log labels say
“encode→display”; the actual measured boundary is packet readiness through the
tablet's render callback and host ACK receipt, not optical display.

## Investigation proposed at observation time

Reproduce retirement arriving around replacement encoder startup using the
normal fake-helper/FIFO test suite. Locate the ordering that lets FFmpeg open a
missing path, add a permanent failing T429 regression, then fix and verify it
without reattaching the user's active display. An encoder-only change should
retain the helper, including recovery from a partial raw frame.

The user's original option/path remains unconfirmed. This observation does not
establish that their earlier searching interval had the same cause. Physical
normal/low-latency/normal transition validation still needs an isolated display
setup avoiding T222; do not deliberately repeat reattachments on the active
Cinnamon session as a regression test.

The subsequent [ownership investigation](2026-09-18-fifo-ownership.md) establishes
the tested cause and verification scope. It supersedes the initial hypothesis
that partial-write retirement itself removed the path.
