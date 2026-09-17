# Architecture and responsibility review — 2026-09-17

Reviewed source snapshot:
[`6a49e9761c541fdcb224bdd6c9216f53581a1c6c`](https://github.com/geraldo-netto/UScreen/tree/6a49e9761c541fdcb224bdd6c9216f53581a1c6c).
This is a review, not a refactor. Findings are recorded in the repository's
[TODO ledger](https://github.com/geraldo-netto/UScreen/blob/configurable-input-devices/TODO.md).
The companion [performance and scalability research](2026-09-17-performance-scalability.md)
describes experiments and their acceptance criteria.

The strongest opportunities are shared session construction, explicit ownership
of capture/decoder lifetimes, separating wire contracts from Linux devices, and
consolidating duplicated encoder/update/packaging policy. Several large modules
already contain short functions; extracting more small helpers alone would not
resolve their mixed responsibilities.

## Scope and evidence

The scan used the tracked-file inventory and examined owned Rust, C, Kotlin,
Python and Shell source, tests, manifests, workflows, packaging and documentation.
Generated Gradle wrappers, the upstream EVDI header, dependencies, binary assets,
caches and build outputs were excluded from code findings. Existing TODOs were
checked to avoid reopening the same issue under another ID.

| Area | Review focus |
| --- | --- |
| `common/` | Configuration policy, persistence, runtime resources, process commands and version comparison |
| `host/src/` | Daemon/session ownership, ADB discovery, capture/encoding, streaming, input, desktop adapters, diagnostics and tray/update integration |
| `host/evdi/` | Owned helper conversion, buffer lifetime, worker pool, FIFO writer and EVDI callbacks |
| `gui/` | Background work, configuration edits, shared policies, process control and rendering |
| `android/` | Activity/Compose responsibilities, transport/input, decoder lifecycle, settings, display controls and service locks |
| `scripts/`, `packaging/`, Make, workflows | Shared staging, platform assumptions, test seams and enforcement of complexity policy |
| Tests and docs | Whether contracts agree across components and whether proposed changes can retain meaningful coverage |

No live daemon, input device, desktop output, service or tablet was changed.
Two isolated probes used production GUI method bodies, the production config
crate, temporary files and a fake `curl`. They establish the limited behaviors
described below, not a live desktop reproduction. Permanent tests belong with
the eventual fixes, with a failing run before the behavior changes.

### Cyclomatic complexity

| Measured functions/methods | Count | Highest score |
| --- | ---: | ---: |
| Rust | 746 | 9 |
| Kotlin/Kotlin build scripts | 249 | 9 |
| C and standalone Python | 228 | 9 |
| Shell/package functions | 30 | 9 |
| Python functions embedded in the publisher | 3 | 3 |
| Total | 1,256 | 9 |

No measured function exceeded the required maximum of 9. This was a local
source-based audit, not a native SonarQube server analysis. Kotlin used the
Sonar cyclomatic visitor; Rust and Python counters followed the inspected Sonar
visitors; C used a syntax-tree branch count. Shell used the explicitly approved
count of one plus branches, loops, case alternatives and short-circuit operators.
Test functions were included. Embedded publisher functions were counted
separately; script-level command sequences are not functions.

The Rust visitor does not expand macro bodies, so a long `tokio::select!` body
is not evidence of low architectural complexity merely because the measured
function passes. The upstream C header produced a parser warning; it is
third-party code and contains no owned helper implementation. These limitations
are why T381 requests a pinned, reproducible repository gate rather than a claim
of complete Sonar certification.

## Merge compatible implementations

| Item | Evidence and proposed boundary | Behavior that must remain explicit |
| --- | --- | --- |
| T368 | `main.rs:run_daemon` and `spawn_extra_session` separately assemble watch/broadcast channels, settings, capture, input and video servers. Introduce one session specification and an owned session runtime, with startup rollback and shutdown. | Primary-only persistence and CLI overrides; extra-slot addressing/card selection; per-session versus whole-daemon stop. |
| T372 | `kscreen.rs:outputs` returns raw JSON, while `input.rs:primary_non_evdi_output` independently fetches/parses the inventory; capture, mapping and diagnostics interpret overlapping fields. Share a typed inventory and a command boundary. | Placement, physical-output preference and diagnostic severity remain different policies. T224/T234/T299 own their existing contradictions. |
| T373 | `encoder.rs:low_latency_options` says options are kept in one place, but `capture.rs:encoder_quality_args` implements the CLI policy separately. Introduce a shared profile translated by each adapter, with capability differences represented explicitly. | CLI wall-clock IDR forcing versus in-process requests, supported formats/depth, initialization requirements and uncapped VAAPI CQP semantics. T259/T284 remain unresolved. |
| T374 | GUI release checks split raw JSON strings, while the daemon uses `serde_json`. Share the release response parser, normalization and endpoint metadata. | Separate sync/async fetching, polling schedules and User-Agent identities. Android can share fixtures, without importing a Rust implementation. |
| T375 | `TouchCapture.emitPen`, `sendPenButton` and `sendPenProximityExit` repeat the pen message fields. Use one typed event serializer after translating each Android action. | Tip, proximity, side buttons, eraser and historical samples carry different meanings; field similarity does not justify conflating them. |
| T379 | `Makefile:dist-local` duplicates the file layout already centralized for release/CI in `stage-linux-bundle.sh`. Parameterize that staging helper for local inputs. | Portable provenance checks, libevdi input validation, APK generation and distro package policies. |

Existing abstractions worth keeping include bounded command helpers, shared
version comparison, config edit merging, encoder frame I/O, Android display
controls and decoder statistics. Similarly shaped loops are not sufficient
reason to merge distinct authentication protocols, scaled/native conversion
kernels or platform-specific package installation rules.

## Split responsibilities and make dependencies explicit

| Item | Current coupling | Proposed direction and important coverage |
| --- | --- | --- |
| T369 | `CaptureManager` combines helper/EDID/FIFO ownership, desktop placement, encoder launch, restart supervision and packet parsing. Stream/input/encoder modules import shared media types through capture. | Move neutral media contracts and the packetizer out first; separate process/frame-source adapters from supervision. Retain generation invalidation, startup cancellation, partial-NAL and cleanup tests. |
| T370 | `InputServer` combines WebSocket/controller state with concrete uinput creation, KWin/X11 mapping and persistent geometry policy. | Wire/controller layer depends on narrow input and settings operations. Linux adapters own devices/mapping. Cross-language fixtures validate actual Rust responses consumed by Android; T247 demonstrates why hand-written lookalike fixtures are insufficient. |
| T371 | The config crate also owns Unix runtime resources, `/proc` identity, UID/token handling and process operations. GUI consumers pull from the same broad surface. | Separate portable policy/serialization and persistence contracts from platform services. Prefer small modules/interfaces before adding crates. This supports the existing Windows plan without selecting a driver architecture. |
| T375 | Android input translation also manages WebSocket generations, authentication, reconnects and pending settings. | A control-session owner, MotionEvent translator and typed wire serializer can be tested independently. Preserve handshake ordering, cancellation and generation guards. |
| T376 | `VideoReceiver` owns socket framing, codec state, Surface handoffs, output threads, watchdog decisions and timing correlation. | A decoder owner and transport/framing boundary with injectable clock/codec operations reduce private-field test coupling. Keep generation checks at every asynchronous handoff. |
| T377 | Activity/Compose callbacks directly mutate preferences, transports, decoder settings and window policy. | Use observable state and explicit events through an activity-scoped coordinator; split settings UI by responsibility. Preserve user preferences, app-switch behavior and lifecycle ownership. |
| T378 | GUI rendering invokes blocking config persistence before dispatching a possible restart. | A settings controller needs structured asynchronous completion, not just an action-result string. Decide how edits made during a save reconcile with the saved baseline, and restart only after successful persistence. |
| T380 | The C helper's mode state, conversion workers and writer exchange buffers through many globals in one translation unit; tests include that whole unit. | Extract explicit conversion/pool and frame-exchange contexts before changing process ownership. Keep buffer retirement and callback lifetime visible, with existing sanitizers and deterministic failure fixtures. |

Applied to SOLID, the evidence is strongest for **single responsibility** and
**dependency inversion**: orchestration depends directly on file, process,
device and codec implementations. Small interfaces for frame sources, encoder
operations, input sinks and settings persistence would also improve interface
segregation and make backend extension easier. No separate Liskov substitution
violation was established. Adding a general plugin framework or an interface
for every helper would create work without evidence of benefit.

## Reproduced inconsistencies

### T374: equivalent release JSON produces different GUI results

Run the production `check_for_update` method with a fake successful `curl`
returning each fixture:

```json
{"tag_name":"v9.0.0"}
{"tag_name":"\u00769.0.0"}
```

These are two separate, semantically equivalent JSON documents. The first
returned `Some("9.0.0")`; the second returned `None`. The daemon's JSON parser
decodes the escape; the GUI's raw quote splitting does not. No network request
was used. Required permanent coverage is the shared fixture pair, invalid JSON,
wrong types, nested lookalike fields and absent fields, exercised by both Rust
callers.

### T378: a config lock blocks the GUI's save method

Create a temporary production config, hold its `config.lock` with `File::lock`,
and invoke the production `App::apply(false)` on a worker. It remained blocked
after 300 ms. Releasing the lock allowed it to return at approximately 307 ms
with “Settings saved.” `show_footer` calls this method inline during GUI work,
so the blocking path reaches the render thread. The probe did not start an
actual GUI, and the timing is evidence of lock dependence, not a performance
benchmark.

The regression must demonstrate GUI progress while the lock is held, then
verify saved/unsaved state, concurrent edits, error presentation and exactly-once
restart behavior after completion. Simply adding a file-lock timeout would not
make synchronous GUI waiting responsive.

### Existing issues remain independently tracked

The review does not duplicate the Cinnamon/Xorg crash (T222), FIFO corruption
risk (T226), orphan process targeting (T245), authentication greeting mismatch
(T247), session coalescing/card assignment (T281/T330), decoder fallback (T339)
or other existing ledger rows. Refactoring can create test seams for them; it
does not resolve their behavior automatically. The performance research adds
the independently verified Wi-Fi API/comment contradiction as T392.

## Suggested dependency order

1. Establish shared wire/JSON fixtures and preserve the existing behavioral
   regressions. Resolve correctness issues such as T247/T226 before interpreting
   performance failures as resource limits.
2. Extract neutral media/config/desktop inventory boundaries (T369–T372), then
   common encoder/session policy (T373/T368). Small update/staging consolidation
   (T374/T379) can proceed independently once their contracts are settled.
3. Separate Android input/decoder/UI ownership and the C worker/buffer contexts
   (T375–T377/T380). Preserve lifecycle behavior before performance experiments.
4. Implement responsive persistence (T378/T385), reproducible complexity checks
   (T381), and the measurement work beginning at T382. Make one commit per
   resolved finding and remove only its resolved TODO row.

Actionable contradictions are **open**. The maintainer clarified this policy
during review: **blocked** is reserved for actual missing decisions, evidence
or prerequisites. Each retained blocked row identifies that obstacle; ordinary
implementation choices do not require a blocked status. Reclassification does
not fix or remove the underlying finding.

## Source entry points

- [Host startup and sessions](https://github.com/geraldo-netto/UScreen/blob/6a49e9761c541fdcb224bdd6c9216f53581a1c6c/host/src/main.rs)
- [Capture and packetization](https://github.com/geraldo-netto/UScreen/blob/6a49e9761c541fdcb224bdd6c9216f53581a1c6c/host/src/capture.rs), [input](https://github.com/geraldo-netto/UScreen/blob/6a49e9761c541fdcb224bdd6c9216f53581a1c6c/host/src/input.rs), [encoder](https://github.com/geraldo-netto/UScreen/blob/6a49e9761c541fdcb224bdd6c9216f53581a1c6c/host/src/encoder.rs)
- [Configuration](https://github.com/geraldo-netto/UScreen/blob/6a49e9761c541fdcb224bdd6c9216f53581a1c6c/common/src/lib.rs) and [GUI](https://github.com/geraldo-netto/UScreen/blob/6a49e9761c541fdcb224bdd6c9216f53581a1c6c/gui/src/main.rs)
- [Owned C helper](https://github.com/geraldo-netto/UScreen/blob/6a49e9761c541fdcb224bdd6c9216f53581a1c6c/host/evdi/evdi_helper.c)
- [Android production sources](https://github.com/geraldo-netto/UScreen/tree/6a49e9761c541fdcb224bdd6c9216f53581a1c6c/android/app/src/main/java/com/uscreen)
- [Bundle staging](https://github.com/geraldo-netto/UScreen/blob/6a49e9761c541fdcb224bdd6c9216f53581a1c6c/scripts/stage-linux-bundle.sh) and [Makefile](https://github.com/geraldo-netto/UScreen/blob/6a49e9761c541fdcb224bdd6c9216f53581a1c6c/Makefile)
