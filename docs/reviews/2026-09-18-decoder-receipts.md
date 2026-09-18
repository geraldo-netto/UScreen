# Decoder transition evidence (T484)

T478's richer decoder requests exposed two transition gaps. The active capture
loop compared the encoder and video format but omitted the decoder request;
changing only the decoder or hints could retain the previous encoder generation.
Selection verification also accepted the old decoder's acknowledgements when
the encoder name and video format matched. Producer-side tagging alone cannot
prove which configuration the consumer used before a control update arrived.

The fix restarts only the encoder when the decoder request changes. Encoder
evidence retains the complete request, and both verification and health checks
require that identity. Android publishes a configuration receipt together with
the successfully started codec. Render ACKs send this receipt while the decoder
ownership check still holds its monitor. Retired codec callbacks cannot send a
replacement's receipt. Watchdog-disabled hints produce a different receipt.

The host rejects missing/mismatched receipts for a richer trial without consuming
the tracked packet. Legacy selections continue using ACKs without a receipt.
This identifies supplied configuration keys, **not** effective Android hints,
successful profile decoding by itself, authentication or optical presentation.
The string format and its cross-language vector are retained in
`testdata/decoder-selection.json`; see [the protocol](../video-codecs.md).

Permanent regressions in the normal suites cover:

- T484 capture comparison: a decoder-only change restarts the encoder, preserving
  the helper geometry.
- T484 selection lifecycle: old ACKs cannot certify the new request; three matching
  replacement ACKs can.
- T484 input contract: old/missing receipts cannot acknowledge a newly produced
  packet; the correct receipt can.
- T484 shared/Android receipt vector, disabled hints, published codec replacement
  and retired callbacks (Android API 27 and 34).

The first three tests failed before the respective fixes and passed afterwards.
[Compressed logs and hashes](2026-09-18-decoder-receipts/) preserve those failures
and successful targeted/full checks. Android's full 374-test suite, lint and
debug APK build passed. Rust workspace tests passed (347 host, 57 shared and
51 GUI; three existing host tests remain ignored). Default and optional
in-process Clippy passed with warnings denied. Complexity checks found no
function above nine. No application was deployed and no physical display was
attached for this fix. T479 measurements remain separate work.
