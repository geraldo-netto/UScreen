# T388 — partial USB comparison with low-latency H.264

**No battery-saving gain is established.** On 2026-09-18, one five-minute
normal/static measurement completed. The subsequent saver/static measurement
lost Linux keyboard focus after about 148 seconds and was rejected. The remaining
paired trials and sustained streaming-off control did not run. T388 remains
pending an undisturbed host session or an explanation of unexpected focus loss.

Three short preflights preceded the sustained attempt: two were rejected, and
the third passed static, motion and streaming-off checks. Those 15-second checks
are diagnostic only and are excluded from power comparisons.

The intended comparison holds the deployed host/APK and current low-latency
H.264 configuration fixed. Installed hashes match the
[live configuration record](../reviews/2026-09-18-low-latency-live/setup.json):
host `52fcd75`, Android runtime source `bdaa17a`, Constrained Baseline/CAVLC,
1280×800 at 60 FPS target, quality 18 and 4 MiB pipe capacity. Brightness remains
50% and display refresh 60 Hz. This is a comparison of the installed optimized
setup, not a deployment of newer checkout changes such as T429.

## Preflight findings

1. The first preflight exposed **T488**, a collector query error. On this
   Android 16 tablet, `dumpsys window windows` lists the UScreen window but
   omits `mCurrentFocus`; `dumpsys window displays` supplies the actual focus
   field. The collector therefore rejected an unlocked, foreground app.
   A permanent command-level regression failed before correcting the query.
   All 15 observation tests then passed, and a physical read-only snapshot
   verified the tablet process and foreground. Commit `491ac6e` contains the
   fix and regression; process, keyguard and timeout checks remain enforced.
2. The second preflight passed Android focus validation, then lost Linux
   keyboard focus about four seconds into its five-second warm-up. The
   visibility guard rejected the run. The cause of that focus change has not
   been established; an undisturbed window or an explanation of
   unexpected focus changes is needed before retrying.

A third preflight, after clarifying that Linux host focus must also remain
undisturbed, completed 15-second static, motion and streaming-off checks with
five-second warm-ups. All three summaries passed their observation and
visibility checks. No guard was relaxed.

All three attempts restored all recorded Android preferences, including the
enabled battery profile. Final observations retained UScreen in the foreground with its
streaming service active and no extra UScreen CPU/Wi-Fi locks. The second
attempt retained host PID 149897, helper 651015, FFmpeg 651155, Cinnamon 1897918
and Xorg 1897166. No host service restart, EVDI attachment or system display-mode
change was requested. No settings were relaxed to accept incomplete evidence.

## Accepted sustained observation

The normal/static trial retained ten charge observations spanning 270.004
seconds. The counter stayed at 4,885,110 µAh and the reported level at 49%, with
battery temperature 31.0–31.1°C. USB power was reported throughout, with advertised
5 V/500 mA maxima. These limits are not measured input watts. Zero observed
charge change at this gauge resolution does not prove zero net battery current.

Encoded output remained 5 FPS for the static scene. Medians of the logged
five-second packet-ready-to-render-ACK window p50/p95 values were 20.85/32.5 ms.
These are host-clock software acknowledgement measurements, not pooled frame
percentiles or optical latency. Median host-pipeline CPU was about 1.0% of one
core; Android app CPU was 10.6%, excluding separate native codec/system processes.
Normal mode held one extra CPU lock and one Wi-Fi lock.

Without a valid saver pair, none of these observations establishes an energy
advantage of the battery profile. Do not substitute historical High-profile
results for the missing matched control. The rejected saver phase is retained
as diagnostic-only data; its ordinary summary contains no accepted results.

The sustained attempt restored the original preferences exactly. UScreen
returned with Battery saver enabled, its service active and no extra CPU/Wi-Fi
locks. Daemon, helper, FFmpeg, Cinnamon and Xorg identities remained unchanged.
The measurement observer also contributes work: current Android session checks
run approximately every half-second, so historical power runs with a different
observer schedule are not equivalent controls.

## Evidence

The [artifact directory](2026-09-18-usb-power-comparison/) retains both rejected
preflights, the accepted short preflight and the sustained attempt, raw guard
and process observations, preference restoration, logs, regression red/green output and the complexity result (3793 functions, none
above 9). `orchestration.py.txt` is the corrected orchestration for a future
run; the first attempt preceded the focus-query correction. Per-phase metadata
records collector source hashes separately from the Git checkout identity.
The accepted sustained trial is separate from diagnostic preflights and rejected
phases. `analysis.json` excludes the rejected phase explicitly.

The incomplete sustained plan remains two balanced repetitions of normal/profile
static/motion, with 30-second warm-ups and five-minute measurements, followed by
a ten-minute streaming-off control. The coarse 9.99 mAh charge gauge may still
make small differences inconclusive. Network, pen and alternative power-source
experiments are deferred separately as T487.
