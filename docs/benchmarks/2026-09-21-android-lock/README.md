# T549: Android lock investigation

Read-only inspection on 2026-09-21 found the tablet awake, with both the
UScreen window screen-bright lock and streaming partial wake lock active.
The system records `mLastSleepReason=force_suspend`; the last wake occurred
191 ms after that recorded sleep. These are the *most recent retained power
fields*, not proof that this was the historical lock reported during camera
validation. The available selected event/system buffers contain no caller or
matching action for that sleep. See [filtered native fields](power-excerpt.txt)
and [collection provenance](provenance.json).

Production `MainActivity.onCreate` sets `FLAG_KEEP_SCREEN_ON`. Its visible
window therefore requests an awake screen while UScreen is foreground.
`StreamingService` separately holds a partial wake lock: that keeps CPU work
running and is not a request to bypass Android's lock screen. Camera background
operation has its own explicit preference and foreground-service lifecycle.
Searching the host, shared configuration and Android production sources found
no power/sleep key injection, `goToSleep`, `lockNow`, `forceSuspend`, ADB power
command or `am force-stop` path.

Development benchmarks are a different lifecycle: `profile-usb.py` and
`decoder-device.py` start their benchmark activity with `am start -S -W`;
`rect-device.py` does the same unless `--keep-process` is supplied. Their target
is the benchmark APK, not the installed UScreen display package. Bringing
another activity forward can remove UScreen's foreground screen-on request.
That is a possible route to the ordinary screen timeout, not evidence that a
benchmark caused this `force_suspend` record. No benchmark or forced-stop action
was run during this inspection.

An attribution needs a naturally occurring event with its exact host action,
time and contemporaneous PowerManager/ActivityManager/keyguard records, or an
agreed physical test window allowing manual unlock. Until then T549 remains
unresolved. Changing wake-lock or authentication behavior without the trigger
would be speculation; no behavioral fix or substitute regression is claimed.
