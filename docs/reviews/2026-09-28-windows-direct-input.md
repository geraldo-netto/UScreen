# T689 non-stylus input contract

The maintainer selected direct mouse mode on 2026-09-28: finger position maps
absolutely to the selected monitor; a tap clicks; explicit right-click and drag
controls are separate from native touch mode. Existing Linux pen-derived pointer
behavior and tests are unchanged. This document describes the implemented adapter
and its event contract; authenticated wire/UI integration remains T673. Windows
input capability remains unavailable until that integration and its acceptance.

`common::direct_input::Config` selects an exact native monitor identity and either
`touch` or `direct_mouse`. `Event` contains a tagged touch or mouse event with
normalized finite x/y, a phase (`down`, `move`, `up`, `cancel`) and either a bounded
touch slot or an explicit mouse button. Unknown fields/types/buttons are rejected.
Pen pressure, tilt, hover and eraser do not belong to this contract.

T673 must negotiate an explicit non-stylus protocol capability before accepting
these events. Keep the existing touch/pen wire messages for compatible Linux
sessions. Map authenticated touch messages into the shared touch policy; introduce
an independently advertised mouse event path and saved mode/monitor preference.
Do not repurpose `input_pointer`, `input_pen` or `pen_only` as mouse enablement.
The Android direct-mouse UI must send absolute move events, a left down/up pair
for a tap, an explicit right down/up pair for right-click, and left down/move/up
for drag. Cancellation, mode changes and control-connection loss retire any held
button. Selecting mouse disables native touch delivery for those finger events;
selecting touch restores existing touch semantics. No hidden pen dependency.

The shared session owns ten touch slots and three explicit buttons. It rejects
invalid transitions before native calls. Touch releases use the last delivered
point; Android cancellation coordinates cannot move a lifted contact. Every
injected touch frame includes the session's remaining contacts. Injection failure
retires the session; monitor snapshot changes require retirement before remapping.
Repeated shutdown retries native cleanup and never reactivates input.

The Windows adapter creates one owned PT_TOUCH synthetic device only for touch
mode, injects bounded native contact frames and destroys that device on retirement.
Mouse mode uses absolute virtual-desktop SendInput coordinates from T672. It checks
for a pre-existing button hold, records only successful button downs and releases
only its recorded buttons. Windows does not provide independent per-device mouse
button ownership through SendInput; concurrent physical mouse use and integrity
boundaries require T522 acceptance. Injection failure is reported; the adapter
never elevates, requests UIAccess or bypasses desktop/session restrictions.

Source contracts: [synthetic device creation](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-createsyntheticpointerdevice),
[pointer injection](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-injectsyntheticpointerinput),
and [SendInput and UIPI](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput).

Tests retain invalid coordinates/slots/JSON, disabled modes, contact/button
transitions, denied injection, cleanup retry, replacement, disconnect-equivalent
retirement and stale monitor snapshots. Native fixtures own their test window and
synthetic device and restore cursor position. These do not establish physical
Android input acceptance, elevated-application support or Windows display support.
