# Windows camera-probe build boundary (T631)

The normal native MSVC and GNU cross-build commands failed at `3bcb11f` in
[run 36259429008](https://github.com/geraldo-netto/UScreen/actions/runs/36259429008):
the camera timing example imported Linux camera modules on Windows.

The example now delegates to a Linux-only module. Other platforms build a small
entry point that exits 2 with an explicit unsupported message. The Linux module
is unchanged except for relative module paths and the entry-point name; its four
existing tests remain and pass. No camera capture was started for these checks.

The Windows workflow permanently checks this boundary with a GNU compile and a
native MSVC execution asserting the unsupported exit/message. The GNU check was
run against the original source after adding the workflow regression, failed,
then passed after restoring the platform boundary. The workflow retains full
workspace testing/linking; it does not omit the example to obtain a pass.

Retained evidence:

- [Original-source failure](artifacts/2026-09-26-windows-development/t631-red.log).
- [Same compile command after the fix](artifacts/2026-09-26-windows-development/t631-green.log).
- [Linux example tests](artifacts/2026-09-26-windows-development/t631-linux.log).
- [Complexity gate](artifacts/2026-09-26-windows-development/t631-complexity.log):
  5,963 functions, none above 9.

Local full GNU linking could not run because the host lacks MinGW's `dlltool`;
the Windows workflow installs the required linker. Native CI results remain
separate from this local compile evidence. Windows capture remains unsupported;
native ACL/lifecycle, scheduling and coverage acceptance remain T493/T583/T497.
