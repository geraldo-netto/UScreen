# USB credential redelivery deadlines (T657)

The original tests accepted eventual redelivery even when the initial five-second
deadline was moved into the past. Permanent normal-suite
`t657_redelivery_waits_for_each_complete_interval` now checks zero elapsed time,
one nanosecond before each deadline, the exact deadline and a complete second
interval using Tokio's paused clock. It also shuts down its owned monitor.

The unmodified baseline passes. Changing either the initial or renewed deadline
from addition to subtraction fails the new regression. Production code remains
unchanged. The corrected final Linux USB campaign catches 112 mutations with 12 unviable
edits and one equivalent survivor. T664 invalidates the older report's extra
"caught" result, which came from an unrelated port-fixture failure. Unviable
edits and the equivalent survivor are not counted as caught. The same final
normal suite passes native Windows (the T655 retained evidence includes this test).

[Evidence](artifacts/2026-09-27-mutation-resume/t657/) retains both selected
mutation failures, their passing baseline, source diffs and the final Linux
campaign report/source hashes. The earlier three-candidate timing probe also
included a separate failed-attachment retry-deadline subtraction survivor, which was caught
by the final broader suite; the selected two-candidate evidence is not represented
as the entire campaign.
