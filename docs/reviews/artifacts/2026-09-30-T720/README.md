# T720 validation

The permanent tests belong to the normal Rust and Android suites. Native
integration uses a private PipeWire server, fake authenticated peers and
synthetic constant PCM; it does not touch desktop defaults or physical audio.

Android `profileCoverage lintDebug` passed all 728 tests, without skips or
failures. All 123 functions in `android/app/src/main/java/com/blent/Audio*`
meet the individual 80% executable-line requirement using JaCoCo counters.
Robolectric native timestamp adapter tests run on API27 and API34; they do not
establish physical device timestamp availability or accuracy.

Both legacy and clocked full host CLI transport directions pass the private
PipeWire fixtures. The stereo fixture verifies advancing graph counters and
native delivery through the host; the microphone fixture verifies clocked PCM
admission and owned worker/node retirement. The synthetic rate tests separately
exercise normalized independent native windows and ±1000 ppm interpolation.

The expanded native fixture initially attempted to reuse an old `pw-cat` producer;
it now retires each owned producer before the next protocol run and bounds native
configuration commands. Counter absence before graph startup is explicitly legal;
clocked transport acceptance requires actual synthetic audio with valid native
counters, instead of requiring timestamps on initial silent warm-up packets.
No existing assertion, test or coverage exclusion was removed or weakened.

Physical duplex/AEC, intentional speech, alternate routes, USB recovery and
acoustic delay remain T720's named device/window prerequisite. No acoustic claim,
saved recording, install or physical route change is part of these results.

Rust `blent-config` and `blent`, with the `native-audio` feature, pass 1,029 tests
with five existing ignored tests. All 114 Linux audio production functions in
`common/src/audio*`, `host/src/audio*` and `host/src/bin/audio*` meet the individual
80% threshold. The unsupported foreign-platform entry point remains excluded by
the existing explicit Linux target classifier, with Windows work retained in the
ledger. This is scoped evidence, not a new whole-project or Windows coverage claim.
The complexity scan counts 7,371 functions, none above nine.

Compressed logs, raw LLVM/JaCoCo counters, per-function reports and source manifests
are retained beside this file, with hashes/commands in `results.json`. Rust used a
fresh instrumented target directory. The only source difference after that Rust
collection was removal of a trailing empty line in `AudioPackets.kt`; Android was
recompiled and its full tests, counters and lint recollected afterwards. Every Rust
source fingerprint remains unchanged. The final private native transport fixtures
were rerun after their complexity-only extraction. No new test is skipped.

After merging the separately committed GPU selector, `cargo check --locked
--workspace --all-features` passes. All scoped audio source and test bytes match
the tested worktree. The merged complexity scan and integration build logs are
also retained; only the TODO movement required merge conflict resolution.
