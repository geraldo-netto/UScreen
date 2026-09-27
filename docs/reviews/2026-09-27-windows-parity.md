# Windows feature-parity audit — T690

The maintainer requires new operating systems to provide the existing supported
feature set through shared domain logic and explicit native adapters. The rule
is persisted in repository `AGENTS.md` and the maintainer's global
`/home/netto/.codex/AGENTS.md`. This is a source audit, not new native acceptance.
Windows remains a preview. The explicit stylus exclusion (T526/T592/T682) and
declined macOS scope (T584) remain; touch/mouse stays planned.

| Linux user-facing capability | Windows implementation boundary | Remaining work |
| --- | --- | --- |
| Daemon lifecycle, GUI launch, per-user startup | Native Windows lifecycle/autostart/tray and ordinary-user regressions exist | Retain current tests; integrated feature lifecycle is tested with each new backend |
| Authenticated USB, app launch, device slots | `host/src/usb/monitor.rs` prepares owned slots; preview adapters cannot capture/inject | T525 is implemented; T522 retains physical USB acceptance, T529/T673 bind real backends |
| Touch/mouse on the selected monitor | Native sink unavailable; existing virtual pointer is pen-derived | T672 mapping, T689 non-stylus input contract/backend, T673 integration, T522 acceptance |
| Extended desktop, resolution/refresh/placement, scaled stream | Driver choice/control interface and capture backend missing | T527/T675/T528/T676/T529/T674; enumeration and dynamic settings must share these adapters |
| Codec/profile selection and fallback | Native pipe adapter supports libx264 only; AMF/QSV recipes absent | T685/T530/T677; T529 must retain shared format negotiation, framing, queue/reconnect and configured slot limits |
| Wi-Fi setup/off, saved-address reconnect, USB preference | Windows CLI rejects Wifi; Linux-only supervisor contains setup/reconnect policy | New open T691; extract shared policy and inject native commands/paths, with authenticated owned-route fixtures |
| Front/rear camera endpoints, manual controls, preview, transforms, background consent, stale-output retirement | Shared camera profile/UI exists; GUI factory and host capture/output select Linux only | New open T692 native-interface research; T693 implementation blocked on its control/ownership interface and distribution decisions |
| CLI overrides and config/UI agreement | Shared grammar accepts video/worker flags that Windows does not apply | New open T694: permanent regression before correcting silently ignored flags; explicitly reject unavailable options and later connect supported settings |
| Adaptive idle and named-decoder ACK admission | Portable policy exists; active cadence control uses Linux helper/FIFO files | New open T695 for portable control boundary and Windows fixtures; activation requires capture/session integration and native evidence |
| Update notification in daemon tray | GUI release check exists on Windows; daemon lacks Linux update watch/menu action | New open T696; share release/version policy and native launch adapter |
| Configurable buffer capacity/conversion work and effective-state reporting | `common/src/platform.rs` marks pipe_capacity/conversion_pool false; GUI hides native controls | New open T697; equivalent bounded Windows controls, without requiring Linux kernel APIs |
| Setup, diagnostics, packaging and installation | Basic dependency diagnostics exist; setup is unsupported and packaging pending | T527/T534/T678 cover driver/dependency setup and distribution; new backend diagnostics must accompany the implementing task |
| Performance and recovery evidence | Native fixtures do not establish physical stream/GPU/tablet behavior | T522/T674/T677/T535 retain the single-PC/tablet acceptance and measurement gates |

The added Open items have work that can start independently: extract shared
policy, implement isolated adapters/contracts, correct CLI failure behavior or
research a native interface. Production integration still names its prerequisites.
T693 is blocked because no Windows camera ownership/control interface has yet
been selected; a need for research alone does not block T692.

Feature parity requires equivalent supported outcomes and settings. It does not
require using EVDI, V4L2, Unix FIFOs, systemd or Linux privilege/path conventions
on Windows. Optional backend mechanisms such as Linux shared-memory transfer do
not prove a Windows implementation or justify a zero-copy claim. Preserve
bounds, ownership, cancellation, correctness and user-controlled capacity in the
native alternative. New exclusions require an explicit maintainer decision.

Evidence inspected: `common/src/platform.rs`, `common/src/cli.rs`,
`common/src/model.rs`, `host/src/windows_main.rs`, `host/src/linux_main.rs`,
`host/src/usb/monitor.rs`, `host/src/usb/preview.rs`, `host/src/pipe_encoder.rs`,
`host/src/capture/idle.rs`, `host/src/update.rs`, `host/src/tray.rs`,
`host/src/windows_tray.rs`, `gui/src/platform/windows.rs`,
`gui/src/status_poll/windows.rs`, `gui/src/camera_settings.rs` and the existing
[Windows milestones](../windows-port.md). Native unavailable flags are retained.

Documentation-only validation checks the ledger schema, unique IDs, relative
links and rule persistence. No tests or coverage thresholds were removed or
weakened, and no runtime-support claim was added.
