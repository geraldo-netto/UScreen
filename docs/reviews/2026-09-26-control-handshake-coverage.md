# T629: physical panel metadata on the real control-open path

A permanent normal-suite regression opens and reconnects the control socket,
asserts authentication precedes resolution, and checks native pixel dimensions
plus physical millimetres. Unknown physical width or height omits both optional
millimetre fields. Earlier fixtures exercised synthesized greetings without
opening a socket with complete physical dimensions, leaving `sendResolution`
at 7/9 executable lines. No production behavior changed.

The full Robolectric suite passes on API 27/34. The per-method JaCoCo gate now
passes all 578 maintained Android methods; no test was removed or weakened.
[Evidence](artifacts/2026-09-26-task-batch/t629/).
