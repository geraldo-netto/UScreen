# Opt-in Android power policy — implementation and lifecycle checks

T388's candidate is implemented, tested and installed, but sustained power
validation is **pending**. The Linux desktop was locked when preparing the
controlled workload: the captured virtual-display image contained the lock
screen rather than the test pattern. No new desktop baseline or battery-saving
percentage is accepted from that state. T388 remains in TODO.md pending an
unlocked session and the matched measurements below.

## Behavior

Android Settings includes **Battery saver**, off by default. It applies
immediately and preserves saved brightness, refresh rate, stream FPS, bitrate,
resolution, quality and encoder/decoder selection. Turning it off restores the
normal lock policy. Normal defaults remain 50% app brightness and 60 Hz; other
apps retain their own/system display settings.

| State | Normal mode | Battery saver |
| --- | --- | --- |
| Visible Activity, active USB stream or pen mode | Existing CPU and Wi-Fi locks | Neither extra lock |
| Active network or unknown route | Existing CPU and Wi-Fi locks | Wi-Fi lock; no extra CPU lock |
| Waiting/disconnected | Existing CPU and Wi-Fi locks | No new lock; release an existing Wi-Fi lock after five seconds |
| Activity stopped/service destroyed | Release both locks | Release both locks immediately |

The visible Activity already uses `FLAG_KEEP_SCREEN_ON`. Android documents this
as an Activity-scoped screen-on request that ceases to keep the screen awake
when the app goes into the background. See
[Keep the screen on](https://developer.android.com/develop/background-work/background-tasks/awake/screen-on).
Removing a redundant partial CPU lock is a policy change, not proof of reduced
whole-device energy while the screen remains awake.

A network reconnect cancels the pending release and immediately reacquires the
Wi-Fi lock if needed. Repeated inactive updates do not postpone the deadline.
A confirmed USB route releases the Wi-Fi lock immediately. Unknown routes retain
conservative network behavior while active; older hosts remain compatible.

The host monitor supplies the selected ADB route to the attachment generation.
Its accepted control lease snapshots `transport: "usb"` or `"network"` into the
initial greeting. Android clears this metadata on connection retirement and
ignores stale-socket callbacks. It never infers the route from `127.0.0.1`, the
charging flag, a physical-device identity or an SSID. Network ADB does not prove
that Wi-Fi is the underlying medium.

The Activity owns and cancels its power observer. Foreground-service startup
rejections do not crash the Activity; promotion failures release existing locks.
The service promotes in `onCreate`, revalidates promotion when processing a
request, and releases locks on destruction or a null restart. These changes
respect Android's separate launch/promotion steps and rejection behavior; see
[Launch a foreground service](https://developer.android.com/develop/background-work/services/fgs/launch).
They do not establish the cause of the older T395 foreground-start timeout.

Hidden statistics no longer run a one-second presentation sampling loop, in
both modes. Showing statistics starts sampling, and hiding them cancels it.
Receiver counters and decoder timing remain available. No automatic frame
skipping, frame-rate reduction or experimental decoder profile is enabled.

## Validation and deployment

The [artifacts](2026-09-18-power-policy/) preserve exact source hashes and source
files, candidate binary/APK hashes, test output, deployment facts and physical
lifecycle observations. The deployed candidate was built from `c1812f6` plus the
archived T388 sources. The implementation is committed separately from resolving
T388, which remains pending sustained measurements.
The Linux release build uses Rust 1.90.0/GCC 12.2.0 and the stock external-FFmpeg
path. Android uses the debug variant, matching the earlier baseline variant.

The normal Android suite passed **276 tests**, with zero failures/errors/skips;
lint and APK assembly passed. Default workspace/native/tooling tests, optional
in-process host tests and both Clippy configurations passed. Their pre-existing
ignored manual tests remain identified in the raw output. The complexity check
found no owned function above nine.

Permanent API 27/34 regressions cover USB lock retention, disconnect hysteresis,
recovery, old-host/unknown-route handling, stale callbacks, Activity observer
cancellation, service rejection, prompt promotion, preference/UI persistence and
hidden/visible statistics. Relevant tests were observed failing before their
behavioral fixes. The final telemetry test was repeated against the original
loop and fixed loop after moving it into the existing Compose fixture; every
assertion was retained. A separate Compose fixture interleaved with plain
Robolectric Activity tests stalled the combined suite, despite passing alone.
Keeping UI tests in the existing fixture resolved that test-order interaction.

Robolectric 4.17's default Wi-Fi shadow references a newer platform type absent
from the API 27/34 test runtime. A narrow test shadow models Wi-Fi lock ownership;
it does not simulate radio power or latency. Physical checks therefore complement,
rather than replace, these deterministic lifecycle regressions.

Both applications were installed and the UScreen user service reloaded.
Cinnamon PID **1897918** and Xorg PID **1897166** were unchanged. Installed Linux
hashes match the built artifacts. Existing H.264 VAAPI selection and display/stream
preferences were preserved. No EVDI module reload or FFmpeg patch was used.

## Physical USB lifecycle observations

On the Ulefone RugKing Pad 2 Pro, Android 16, the updated app remained PID 11883
through the toggle/background/resume sequence. Android's dumps confirmed:

| Check | UScreen partial CPU locks | UScreen Wi-Fi locks |
| --- | ---: | ---: |
| Normal USB mode | 1 | 1 |
| Enable battery saver | 0 | 0 |
| Background the app | 0 | 0 |
| Resume with battery saver saved | 0 | 0 |
| Disable battery saver | 1 | 1 |

The active display mode stayed at the recorded mode ID and app brightness was
0.5. The foreground service remained active during visible streaming. The final
setting was restored to normal/off. These are ownership/lifecycle observations,
not a power or playback benchmark. The displayed source was the locked Linux
session, so it cannot substitute for the original static/motion workload.

## Remaining measurements

1. Repeat the original 1280×800, 60-FPS H.264 VAAPI static/motion sequence in normal
   mode, with the same USB source, brightness, refresh and APK variant. Check
   workload visibility throughout, not just monitor geometry (T424).
2. Repeat with battery saver and alternate order where practical. Record charge
   endpoints, duration, temperature, observed refresh, process continuity,
   delivered FPS and latency-window distributions. A disabled-lock count is not
   a battery-life estimate.
3. Add an app-off static-image control at matching brightness/refresh, plus
   waiting, pen-only, background and reconnect observations. The separate
   `power-project.py` control APK avoids streaming/decoding work. Its prepared
   image/APK was rejected because the screenshot contained the lock screen;
   recapture verified workload pixels and rebuild before use.
4. Evaluate available network transport and any physically available higher-power
   data port separately. Preserve the actual link/power-source facts; a charging
   flag or advertised limit is not a USB wattmeter.
5. Measure the optional low-latency H.264 profile separately, so its changed
   bitstream does not confound the normal/profile power comparison.

The original [4490763 baseline](2026-09-17-device-baseline.md) lost 189.81 mAh
over 35.5 minutes despite USB power. Its gauge moves in 9.99 mAh steps, making
short comparisons weak evidence. New power savings remain unmeasured; component
latency improvements from the [codec experiments](2026-09-18-codecs.md) cannot be
added to claim a total capture-to-display or battery improvement.
