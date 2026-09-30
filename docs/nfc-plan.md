# NFC tag reading scope and implementation plan

T546 research supports a read-only tag-to-computer feature using Android reader
mode and the existing authenticated attachment. Blent does not implement NFC
sharing yet. The remaining product choice is whether standard NDEF records meet
the intended use case, or a specific non-NDEF tag/protocol is required. T740
retains that decision before implementation; this document proposes a concrete
first version and its dependent work.

## Verified platform capabilities

On September 30, read-only queries against the connected RugKing Pad 2 Pro,
Android API 36, returned `android.hardware.nfc`, `android.hardware.nfc.any`,
`android.hardware.nfc.hce` and `android.hardware.nfc.uicc`; `service check nfc`
reported the NFC service. This establishes advertised hardware/service presence,
not enabled reader state or successful tag reads. No NFC setting or tag was
changed. Camera prerequisites T621/T540 have already passed.

Android's reader mode is available from API 19 and is limited to a foreground
Activity. Reader mode temporarily changes NFC operation, including card-emulation
availability, so the adapter must disable it when sharing stops or the Activity
pauses. Capability reporting must distinguish absent hardware, disabled NFC and
an active session. [NfcAdapter reference](https://developer.android.com/reference/android/nfc/NfcAdapter#enableReaderMode(android.app.Activity,%20android.nfc.NfcAdapter.ReaderCallback,%20int,%20android.os.Bundle)).

The `Ndef` API reads structured tag messages off the main thread. It can return an
empty message or fail when a tag leaves, a read is malformed or IO is cancelled;
closing its connection from another thread cancels a blocked read. Those outcomes
need separate states and cleanup. [Ndef reference](https://developer.android.com/reference/android/nfc/tech/Ndef#getNdefMessage()).

NDEF message bytes can be serialized for transport without executing their
contents. Android's serialization normalizes record encoding, so it should be
described as record delivery, not a byte-identical dump of physical tag memory.
[NdefMessage reference](https://developer.android.com/reference/android/nfc/NdefMessage#toByteArray()).
Non-NDEF technologies need their own protocol handling; tag writing and card
emulation are separate operations, outside the selected read-only use case.
[Advanced NFC overview](https://developer.android.com/develop/connectivity/nfc/advanced-nfc).

## Proposed first version

The computer's NFC controls explicitly request Start/Stop for the selected tablet.
The tablet exposes its active reader session and Stop action while Blent is
visible. Launch, reconnect, settings Apply and returning from the background do
not silently restart scanning. Stale callbacks cannot publish into a replacement
session, and a replacement waits for native retirement, following the ownership
contracts already used by camera/audio sharing.

Deliver standard NDEF records to a read-only GUI list and an explicit CLI JSON
output. The GUI offers deliberate Copy/Save actions. Preserve record type,
identifier and payload as bounded binary fields; decoded text/URI previews are
optional interpretations, with raw fields retained. Never open a URI, execute
content, inject keystrokes or log tag payloads automatically. Clear pending records
on Stop/disconnect; saving is an explicit user action. A generic PC/SC or USB NFC
device is not supplied by Android reader mode and is not promised by this plan.

Proposed configurable limits are 64 records and 64 KiB serialized payload per
message, an eight-message queue and a two-second read deadline. Reject a message
atomically if a bound is exceeded; report overflow rather than silently presenting
a partial message. Check Android's message length before serialization and enforce
independent receiver bounds. Framework-created tag objects are not an application
allocation guarantee. Native timeout/close behavior must be tested on the tablet.

## Implementation dependencies and acceptance

1. **T740:** choose the proposed NDEF-to-GUI/CLI scope or name the required
   non-NDEF technology and desktop consumer. The latter needs a revised protocol
   contract before native operations are designed.
2. **T741:** add shared bounded records, capabilities, authenticated generation and
   sequence handling, explicit Start/Stop and queue policy behind native reader
   and delivery interfaces. Keep UI, configuration and wire contracts independent
   of Linux device paths. Retain malformed/truncated/oversized/property tests and
   rejected-state atomicity before connecting native IO.
3. **T742:** add the Android foreground reader and host GUI/CLI delivery using
   T543 controls and T741 policy. Advertise unsupported backends explicitly. Test
   absent/disabled adapters, permission/enablement failures, empty/unsupported
   tags, tag loss, bounded IO, repeated replacement, Stop, pause and disconnect.
   Require individual 80% function coverage and complexity no greater than nine.
4. Physical acceptance needs identified NDEF fixtures and a brief host/tablet
   window: compare all delivered records, remove a tag during reading, Stop and
   reconnect, and verify reader retirement without writing or formatting a tag.
   API compilation and synthetic fixtures alone cannot close native acceptance.
