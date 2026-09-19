# T321: conservative minimum display timing

UScreen now accepts generated DTD clocks from **10 MHz through 655.35 MHz**,
using the encoded 10 kHz units and the existing nearest-unit rounding. This is
a conservative supported-timing boundary. It does not establish a kernel failure
below that clock or compositor compatibility above it; no live EVDI attachment
was attempted.

At 640×480, fixed totals of 800×518 require at least **25 Hz**. The previous
10 Hz mode encoded 4.14 MHz. Both Rust and Python generators now reject it,
report the higher-refresh alternative, and advertise 25 Hz as that geometry's
minimum range. Generation 7 prevents reuse of earlier EDID cache names.

The fallback and rejection paths have distinct purposes:

- Explicit CLI, GUI and Android FPS edits reject unsupported joint timing and
  preserve the previous saved/live values. Scalar settings retain their existing
  bounds normalization; that normalization cannot silently repair a low-clock
  explicit mode before validation.
- Loading an old saved configuration or negotiating native geometry raises a
  low refresh to the first supported integer value while preserving dimensions.
  Thus 640×480 at 10 becomes 25 FPS. Confirmed settings reach Android through the
  existing response path. Excessive clocks remain errors; no second display/stream
  FPS control was introduced.

Permanent tests failed before their corresponding changes for shared validation,
saved settings, both generators, native negotiation, explicit save/CLI requests,
and the advertised range minimum. The retained T332 parity case now rejects
640×480 at 10 and accepts it at 25. T115 keeps its one-pixel-width packing case at
a supported 60 Hz and also asserts rejection of its formerly accepted 10 Hz case.

The normal suite includes 9,657 deterministic Rust numeric cases and a Python
corpus of 1,048 random/boundary cases, including negative Python values, zero,
32/64-bit extremes, invalid physical sizes, pixel-clock rounding and invalid
refreshes. Accepted EDIDs retain valid lengths/checksums, and invalid CLI generation
does not overwrite an existing file. The Python corpus runs through Cargo tooling.

Validation: 397 daemon tests passed (three existing opt-in tests ignored), 65
shared-library and 51 GUI tests passed; a subsequent eight-test storage run also
includes the new missing/malformed-file fallback regression. Generator parity and
the normal Python tooling entry pass. All workspace targets/features pass Clippy
with warnings denied. Formatting and the complexity gate pass: 4,146 functions,
none above nine. The full MSVC workspace checks, and 24 portable Windows policy
tests pass in isolated Wine; neither result resolves T493's native ACL blocker.

All four shared timing functions, both configuration sanitation methods, Rust
EDID generation, native geometry negotiation and the CLI merge measured **100%
executable-line coverage**. The four maintained Python generator functions also
measured 100%. The scoped inventory includes adjacent save/validation functions;
all exceed 80%. This is not project-wide coverage completion; T497 remains open.

[Compressed evidence](2026-09-19-supported-timing/) preserves red/green logs,
coverage and scoped function counts. No display, latency or battery improvement
is claimed from these validation checks.
