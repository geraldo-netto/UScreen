# T395 — Android foreground-service startup investigation

The original `ForegroundServiceDidNotStartInTimeException` remains unresolved.
Thirty controlled starts on the affected RugKing Pad 2 Pro / Android 16 did not
reproduce it. These observations do not establish the original cause or justify
removing T395 from the ledger.

## Method and results

The [artifacts](2026-09-18-foreground-start/) preserve APK hashes, source revisions,
launch scripts, service/PID observations, bounded lifecycle traces and test logs.
The historical `29b8068` and current `4617517` Android sources were built as a
separate `com.uscreen.lifecycleprobe` application. Only the application ID and
lifecycle logging were changed; the instrumented sources are archived. This
protects installed preferences and distinguishes isolated trials from live ones.
The probe used a synthetic token, so its starts did not establish authenticated
streaming. Both builds used the existing debug toolchain and signing key.

Each isolated build ran two repetitions of six cases: plain cold start,
concurrent launcher/token Activity starts, token-first start, immediate Home,
Home/resume and rotation. Each trial force-stopped only the probe, sampled
service state and PID eleven times, and retained logs. Original rotation settings
were restored. Six additional cold starts used the installed UScreen APK with
its real host connection: plain start, concurrent token Activity, token broadcast,
Home/resume, immediate Home and repeated launcher intent. No real token appears
in the artifacts. Installed app preferences were identical before and after.

| Source | Trials | Observed timeout | First observed foreground service after launch |
| --- | ---: | ---: | --- |
| Historical source, isolated package | 12 | 0 | 2.37–2.80 s |
| Current source, isolated package | 12 | 0 | 1.57–2.58 s |
| Installed app, live host | 6 | 0 | 1.79–3.10 s |

Times are coarse shell/poll observations, including process startup and sampling
overhead; they are not exact promotion timings or a performance comparison.
Immediate-Home cases did not leave a foreground service. No trial showed an
unexpected replacement PID after the first observed process. The installed app
returned to streaming. Linux service, capture helper, Cinnamon and Xorg were not
restarted for these trials.

The tablet's secure keyguard remained active, with UScreen/probe visible above
it through the production Activity flags. This matters for reproductions: an
unrelated Activity without those flags can be paused immediately (T460).

## Existing protection and remaining prerequisite

The current service promotes itself in `onCreate` as well as checking promotion
in `onStartCommand`. Its Activity-owned observer stops on backgrounding;
promotion/start rejection releases locks or stops observing. The permanent
`StreamingPowerTest` and `StreamingBindingTest` suites passed on API 27 and 34,
covering early promotion, rejection, cancellation/re-entry and lock release.
Those tests already accompany T388's implementation. No speculative production
change or redundant regression was added here.

Android documents this exception as a failure to promote a service within the
foreground-start deadline. See the [official troubleshooting guidance](https://developer.android.com/develop/background-work/services/fgs/troubleshooting).
The original incident does not have enough preserved scheduling evidence to
identify which startup/lifecycle operation consumed or cancelled that deadline.
The isolated historical build also differs from the original package identity,
fresh-install state and authenticated streaming sequence.

T395 is blocked on a reproducible failing sequence or a new timestamped failure
trace identifying that operation. On recurrence, preserve Activity/service
lifecycle, main-thread scheduling, token-delivery order, process identity and
`ApplicationExitInfo` before retrying. Add a deterministic failing regression for
the demonstrated cause before changing behavior. Repeated successful starts do
not prove an intermittent failure has been fixed.
