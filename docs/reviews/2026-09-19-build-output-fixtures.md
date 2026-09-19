# T508 — build-output fixture signing prerequisites

T336's `make dist-local` cases selected the intended artifacts but failed at
T250's signing verification because their isolated PATH lacked the offline
signing tools. The shared fixture now supplies the designated certificate and
package responses. Production verification and artifact assertions are unchanged.

The permanent suite failed in three subcases before the fix and passed all
three tests afterward. See [red/green evidence](artifacts/2026-09-19-build-output-fixtures).
