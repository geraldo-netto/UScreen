# T691 shared Wi-Fi setup and reconnection

Windows preview now admits `blent wifi` and `blent wifi --off`. Both platforms
share setup, saved-address handling and reconnect success parsing through injected
ADB/configuration adapters. Setup requires an authorized USB device with the Blent
app, switches Android ADB to port 5555, discovers its IPv4 address and saves only
a confirmed connection. Host video/input listeners remain on loopback. `--off`
forgets the saved address and attempts disconnect; it does not disable the tablet's
ADB listener or globally prohibit independently discovered network transports.

The Windows attachment monitor accepts authorized network and mDNS inventory,
prefers USB for proven physical identities and keeps unknown identities distinct.
Existing authentication, reverse-route ownership, app launch policy, configurable
slot/port limits and cleanup remain shared. Migration retires credentials and
listeners before replacement. Proven aliases allow cleanup when the former
transport is offline. A different physical identity cannot inherit old route
ownership; cleanup debt waits for a matching identity without reserving a live
session. A transport switch does not relaunch an already observed tablet.

Reconnect reloads the saved address, backs off attempts and checks again after
ADB returns; an address disabled/replaced mid-command is disconnected. Active
connections avoid unnecessary reconnect attempts. Malformed or unavailable
inventory preserves existing sessions; unauthorized devices cannot enter the ready
set. Inventory is bounded to 256 observations and network discovery has a six-
second poll budget. A timeout cancels owned command work and preserves the last
confirmed session state. CLI and tray distinguish network ADB from USB while
continuing to state that capture and integrated input are unavailable.

Permanent tests cover false success text, setup/off, missing tools, invalid
addresses, unready devices, live configuration changes, network/USB migration,
physical identity replacement, owned route cleanup, one-time launch, authentication
and inventory bounds. Native fixtures use an owned ADB executable, real sockets and
isolated configuration stores. They do not establish real Windows/tablet Wi-Fi,
USB or device acceptance; T522 retains that physical prerequisite. No performance
or battery improvement is claimed and T382 remains closed.
