# T726: Bluetooth distortion evidence

Read-only review of the local PipeWire and WirePlumber user journals on September
30, 2026. The retained [bounded excerpt](bluetooth-errors.txt) replaces the host
name and headset address with stable labels. No audio content was collected.

The headset node changed from running to error with a Bluetooth transport failure
at September 29 09:59:22 and 17:14:53, and September 30 12:11:48, Europe/Rome
(UTC+02:00). This extends the [earlier snapshot](../2026-09-29-audio-speakers/README.md),
whose short observation had no error. Neither observation establishes the cause
of the reported audible distortion or a relationship to Blent.

The user has been asked whether an audible event matches these times, which
application was playing, and whether distortion still recurs. That correlation
or an agreed recurrence window is the remaining prerequisite. An idle snapshot
cannot reconstruct an earlier audible event. Preserve the same content, route
and workload for a bounded comparison when evidence is available; do not claim
that changing profiles, volume or services fixes an unconfirmed cause.

No system defaults, application routes, volume, codec or service state changed.
No behavioral fix is proposed, so there is no fabricated regression test. A
confirmed Blent defect must first be reproduced by a permanent failing test.
