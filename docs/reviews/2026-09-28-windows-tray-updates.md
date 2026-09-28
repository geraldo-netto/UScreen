# T696 Windows daemon release notifications

The Windows daemon now honors the saved `check_updates` preference independently
of the GUI. Its owned subscription waits 30 seconds after startup, then checks
once per 24 hours. A disabled preference creates no polling task. Ordinary
shutdown cancels and joins the task; owner drop cancels early-exit work. Cancelling
an active check retires its owned process tree through the existing native job or
process-group adapter.

Linux and Windows share polling/version policy and a byte/time-bounded command
reader, also reused by encoder inventory probes. Release replies are capped at
1 MiB and execution at five seconds; curl itself receives a four-second timeout.
Malformed UTF-8/JSON/SemVer, oversized replies, missing curl and offline failures
preserve the last validated notification. A valid current/older version clears
it. Two permanent failing-first regressions cover previously unbounded valid JSON
and invalid tags erasing an existing notification. Release tags are capped at
128 bytes; no supplied URL or command is executed.

Windows resolves curl and its URL launcher from the native system directory.
Its existing native tray receives coalesced release updates beside connection
state, shows an update label in the tooltip/menu, and opens only the fixed project
release page after a user action. Invalid/absent notifications and stopping state
cannot invoke that action. Settings, Quit, Explorer recreation, native resource
ownership and the explicit display/input preview labels remain unchanged.
Nothing downloads or installs an update; existing GUI checking remains separate.

Permanent tests run an owned executable at a Unicode/space-containing path,
verify exact release-page arguments, native menu/notification updates, malformed
responses, disabled polling, deadline/size rejection and in-flight child cleanup.
Native action tests substitute only the executable path and do not launch a
browser or contact the release API. Retained encoder probes verify the extracted
bounded reader through real FFmpeg calls. Existing native tray lifecycle and
resource-counter regressions remain in the normal suite.
