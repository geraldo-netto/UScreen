# T461 decoder-error fixture

The first three complete access units of the T422 `vaapi-cbr` experiment,
1280×800 8-bit HEVC Main, generated from Blent's synthetic motion corpus with
stock FFmpeg 6.1.1 / Mesa 26.2.2 on Navi 23. No encoded bytes were modified.
The third picture emits `cu_qp_delta 99 is outside the valid range [-26, 25]`.
Default FFmpeg software decoding returns three raw frames and exit zero despite
that error. Strict decode validation must reject it before measuring quality.
The first access unit alone is a valid positive control in the same regression.

Full source command, original fixture hash and trial evidence are retained in
[the T422 artifacts](../docs/benchmarks/2026-09-18-hevc-interop.md).

SHA-256: `4e263ec8dc4dfa8f92ccb5697e9b10bb2b5297b2ba8262440e61b3977bab18aa`.
