# T692 Windows virtual-camera adapter contract

This is an implementation proposal for T693, not an implemented or validated
Windows camera backend. The current Linux pipeline is in `host/src/camera.rs`,
`host/src/camera/outputs.rs` and `host/src/camera_control.rs`; the portable
`CameraBackend`/`CameraProfile` contracts and Android consent remain authoritative.
Windows capability reporting stays unavailable until native acceptance.

## Native interface

Use Windows 11 build 22000 or later, matching the existing Windows target.
Microsoft's [MFCreateVirtualCamera contract](https://learn.microsoft.com/en-us/windows/win32/api/mfvirtualcamera/nf-mfvirtualcamera-mfcreatevirtualcamera)
provides software-camera registration using a custom media-source CLSID. Its
current-user access scope does not require administrator privileges; all-user
creation does. Camera privacy denial must remain an explicit failure. Invoke
creation on a worker, never the GUI thread. A successful object creation is not
proof of endpoint enumeration or consumer delivery.

The [Microsoft virtual-camera sample](https://github.com/microsoft/Windows-Camera/tree/master/Samples/VirtualCamera)
provides a software source and a COM DLL installation example. Its MSI registers
the DLL under HKLM. This is a user-mode camera implementation, distinct from the
older stub-driver registration route described in Microsoft's
[Frame Server custom-source guidance](https://learn.microsoft.com/en-us/windows-hardware/drivers/stream/frame-server-custom-media-source).
Do not infer that current-user endpoint access allows per-user COM registration:
Frame Server activates the source across a service boundary.

## Shared policy and native ownership

Extract the Linux-independent camera invitation, authenticated bounded transport,
decode, lens routing, transforms, freshness and preview loop into a shared session
runner. Inject an output factory; keep V4L2 probing, FFmpeg output processes,
Windows COM, handles, ACLs and installation entirely inside their adapters.
The existing `CameraBackend::start/stop/state/preview` remains the GUI boundary.
Saving preferences never starts capture. Display sharing remains independent.

The Windows adapter owns two distinct endpoint identities, Blent Front and Blent
Rear (Windows appends its virtual-camera label). Identify each by product CLSID,
current user and lens, not by friendly-name matching. Creation is transactional:
if the second endpoint fails, remove only a first endpoint newly created by this
attempt. Existing owned registrations survive a failed session startup. Reject a
foreign/conflicting registration; never reset other cameras.

Proposed control operations are `open_pair(owner, profile)`,
`publish(owner, lens, generation, sequence, frame)`, `black(owner, lens)` and
`close_pair(owner)`. Opaque native handles never cross the adapter boundary.
Keep endpoint registration separate from a manual capture session. Use
[current-user system-lifetime registrations](https://learn.microsoft.com/en-us/windows/win32/api/mfvirtualcamera/ne-mfvirtualcamera-mfvirtualcameralifetime)
so owned endpoints can remain enumerable across controller shutdown/reboot, like
Linux's provisioned V4L2 nodes. Normal Blent Stop revokes the frame generation
and publishes black; it must not call
[`IMFVirtualCamera::Stop`](https://learn.microsoft.com/en-us/windows/win32/api/mfvirtualcamera/nf-mfvirtualcamera-imfvirtualcamera-stop),
which disables enumeration and invalidates active consumers. Native Stop/Remove
are explicit endpoint disable/uninstall operations, scoped to owned registrations.
`close_pair` retires the capture lease and handles without deleting registrations.

The Frame Server source emits black when no authenticated controller exists,
a lens is inactive, or the latest frame reaches two seconds of local receive
age, matching `host/src/camera/outputs.rs::FRESH_FRAME_AGE`. It must enforce this
independently of the interactive process, including controller crashes. Host
Stop, transport loss, malformed data and generation replacement invalidate queued
imagery immediately. Consumer activation must never invite Android capture.
Only the selected lens receives fresh frames; opening an endpoint cannot resume
capture. Real consumer acceptance must verify that Stop/crash yields black,
rather than leaving the final live frame displayed.

The custom COM source implements the required Media Foundation source/stream
interfaces and sample/event lifecycle. Negotiate explicit bounded formats;
initially use CPU-owned NV12 samples, converting the shared decoded format in
the native adapter. Include YUY2 only with tested conversion and consumer demand.
Own all buffer lengths, row strides, timestamps, pending sample tokens and COM
references. Stop and shutdown must retire pending requests and release resources
without waiting indefinitely for a consumer.

Frame Server runs outside the interactive process. Design its data channel as a
versioned, bounded IPC interface with a fresh owner generation and authenticated
handshake. Grant access only to the owning user and the required native service
identity; verify client identity and reject remote named-pipe connections.
Do not grant Everyone access or assume the service shares user-session globals.
A single latest-frame slot per endpoint and bounded outstanding sample requests
prevent unbounded queues. A local monotonic receive deadline decides freshness;
Android timestamps alone cannot establish it. Native acceptance must verify the
actual service token/namespace before selecting the final IPC ACL.

## Distribution decision pending

Recommended route: an optional, versioned x64 camera add-on containing Blent's
own media-source DLL, explicit installation/uninstallation, product-specific
COM registration, hashes, attribution and signed release artifacts. A one-time
elevated install registers protected machine-wide COM binaries. Ordinary
Start/Stop and current-user endpoint operation remain unelevated. Pin interface
and binary versions; diagnose a missing/incompatible component without changing
system state at daemon launch. Installer removal must target only owned CLSIDs,
files and endpoints and preserve user preferences.

The maintainer must accept that separate component/elevated setup route or select
a packaged deployment investigation before T693 installation work. This does not
choose the general application installer under T534, grant permission to install
on the live host, or establish a signing identity. Release signing identity and
installer integration remain explicit distribution prerequisites.

## Required permanent acceptance

Test shared session policy independently, including consent denial, malformed and
oversized frames, stale generation/sequence, two-lens routing, transforms, stale
to black, explicit restart, reconnect and shutdown. Test the native COM source
with synthetic patterned frames before using a physical tablet. Native tests
must cover privacy denial, missing registration, partial creation, foreign
ownership, service loss, consumer loss, Stop/Start replacement and bounded cleanup.
Require at least 80% executable-line coverage for every maintained function,
including callbacks, with native Windows counters; enforce complexity at most 9.

Finally validate real enumeration and output in Windows camera consumers,
concurrent preview, black retirement and current-user isolation, plus the existing
Android permission/background lifecycle on a separately authorized camera window.
Neither the development VM nor interface tests establish physical camera parity.
