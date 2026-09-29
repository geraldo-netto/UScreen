# T715 validation evidence

September 29, 2026, Linux/Android. Local times in `phases.json` are CEST; host log
messages use UTC. `deployment.json` identifies read-back-verified artifacts. The
source base is 4797820 plus the T715 commit containing this directory.

- `red.log.gz`: permanent delayed-native-start regression fails at 13 seconds.
- `red-android.log.gz`: permanent transition-frame regression fails before the
  connection overlay's exit fade is removed.
- `coverage.log.gz`: 66 Linux selection tests pass, including all new regressions.
- `coverage-report.json.gz`: 20/20 maintained functions in worker.rs >=80%.
- `android-check.log.gz`, `android-coverage.json.gz`: 622 Android tests pass,
  lint/assembly pass, 653/653 production functions >=80%.
- `complexity-final.log`: 6,887 functions checked, none above nine.
- Raw LLVM/JaCoCo counters and source manifests support those scoped results.
  `rust-counter-reuse.txt` records identical Rust hashes across the Android-only
  edit; counters from different Rust revisions were not merged.

Native checks:

1. Initial installed T714 code wrongly invalidates good tuning before EVDI starts;
   `native-events.log.gz` retains selection, timeout and helper chronology.
2. Fixed host restart selects the historical winner without offline probes or
   comparative trials and receives actual Android render ACKs.
3. Final signed APK removes the exit-frame flash. `ui-summary.json` contains no
   waiting-screen sample in any final calibration/completion/reconnect phase.
4. Clicking GUI Recalibrate display and Apply & restart changes saved generation
   from zero to one; a new profile is measured and retained.
5. Replacing that owned saved record with an invalid fixture triggers fresh
   calibration and a valid replacement record. `profile-before-corruption.json`
   preserves the prior record; no invalid fixture is left installed.
6. Final host restart reuses the recovered profile; Android force-stop/relaunch
   also reuses it without helper replacement or a comparison matrix.

`ui-fixed-restart.jsonl` is intentionally pre-Android-fix evidence and includes the
exit-frame glitch. Other `ui-final-*`, `ui-manual-recalibration`, `ui-invalid-profile`
and `ui-app-reconnect` files are final-APK observations. UI hierarchy sampling is
coarse (about 2.6 seconds), complemented by permanent frame-level Compose tests.
No desktop/camera images are retained here. Normal lock/preferences are preserved.

Native reboot is still unobserved. T715 remains blocked on its original next-
ordinary-reboot condition. The work does not claim to fix libevdi's startup scan
(T558), establish optical latency, or validate Windows display support.
