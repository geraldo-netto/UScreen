# T507 — portable package fixture signing tools

The T235 path-quoting and T428 source-cache success cases failed with `Android
SDK missing` after T250 introduced the required release certificate gate.
Their shared fixture now supplies the same offline signer/aapt responses as the
other release fixtures. Production signing checks remain unchanged.

The existing permanent suite failed in five subcases before the correction and
passed all six tests afterward. Logs are retained in
[artifacts/2026-09-19-package-fixtures](artifacts/2026-09-19-package-fixtures).
