# T689/T673 non-stylus input contract and preview

The maintainer selected direct mouse mode on 2026-09-28: finger position maps
absolutely to the selected monitor; a tap clicks; explicit right-click and drag
controls are separate from native touch mode. Existing Linux pen-derived pointer
behavior and tests are unchanged. T673 integrates the adapter into authenticated
USB/Wi-Fi session ownership and Android controls. Windows touch/mouse is an
opt-in preview; physical tablet acceptance remains T522. Display streaming and
stylus are unavailable.

`common::direct_input::Config` selects an exact native monitor identity and either
`touch` or `direct_mouse`. `Event` contains a tagged touch or mouse event with
normalized finite x/y, a phase (`down`, `move`, `up`, `cancel`) and either a bounded
touch slot or an explicit mouse button. Unknown fields/types/buttons are rejected.
Pen pressure, tilt, hover and eraser do not belong to this contract.

The host advertises `direct_input` with `protocol: 1`, the selected monitor/mode,
allowed `touch`/`mouse` flags and `negotiated: false`. Android sends
`{"type":"direct_input","command":{"type":"negotiate","version":1}}`.
Only the acknowledged controller can then send events or select a mode. A new
controller retires the previous owner's input before its greeting. Existing
touch/pen wire messages remain compatible with Linux sessions. Negotiated Windows
touch maps existing touch messages into the shared policy; mouse uses an independent
`direct_input` event command. `input_pointer`, `input_pen` and `pen_only` never
enable mouse injection.
The Android direct-mouse UI sends absolute move events, a left down/up pair
for a tap, an explicit right down/up pair for right-click, and left down/move/up
for drag. Cancellation, mode changes and control-connection loss retire any held
button. Selecting mouse disables native touch delivery for those finger events;
selecting touch restores existing touch semantics. Mode selection is saved before
acknowledgement and restored on reconnect. A failed negotiation send cannot publish
an authenticated Android connection. There is no hidden pen dependency.

## Using the input preview

In Windows Settings, enable **Touch/mouse input preview**, select an exact target
monitor and choose Touch or Mouse. **Allow touch** and **Allow finger mouse**
control which modes Android may select. Save and restart after changing host
settings. Input is disabled by default until a monitor is explicitly selected.
The preview uses the tablet's whole input surface without video; its Touch/Mouse,
Right-click and Drag next gesture controls sit above that surface. A tap clicks;
moving a finger only moves the pointer unless drag was explicitly armed.
Right-click uses the last valid finger position, so move or tap first.

`blent doctor` lists native monitor identities. The same saved preferences can be
set in `blent/config.toml` under the user's roaming application-data directory:

```toml
input_touch = true
input_mouse = true

[direct_input]
monitor = '\\.\DISPLAY1' # Use the exact identity reported by doctor.
mode = "direct_mouse"    # Or "touch".
```

Remove `[direct_input]` to disable the preview. Restart after host edits;
Android mode changes persist immediately. Monitor layout, rotation or scale changes
retire active input before reconnecting and remapping. A missing selected monitor
or denied injection closes the control session; input never silently moves to
another monitor. Pen messages are rejected. CLI/GUI diagnostics report this
capability separately from the full input backend and from physical acceptance.

## Ownership and validation

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

T673's [native/session regressions and measured coverage](artifacts/2026-09-29-t673-input/README.md)
retain the integration results and the two Android failing-then-passing regressions.
