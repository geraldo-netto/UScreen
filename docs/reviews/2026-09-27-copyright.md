# Blent contribution notices (T662)

`COPYRIGHT.md` adds Copyright (c) 2026 Geraldo Netto for original Blent
contributions: new work and changes to inherited files. It records the inherited
upstream revision, keeps attribution for moved/split code and leaves third-party
terms intact. The holder matches the fork's Git author identity.

The original MIT `LICENSE`, DisplayLink header and LGPL text are byte-identical
to their previous versions. The MIT license and DisplayLink header also match
the inherited upstream revision directly. The LGPL text was added later as
third-party distribution material and is not attributed to the fork author.

The shared distribution manifest carries the new notice in portable, AppImage,
Debian, RPM and Arch documentation; RPM marks it as license material. The existing
normal notice test now also requires COPYRIGHT.md. Actual isolated tar/AppImage,
RPM and Arch notice checks pass, as do packaging tests and all five existing
identity checks. No artificial behavioral test was added for prose.

[Retained evidence](artifacts/2026-09-27-copyright/) includes packaged-notice tests
and immutable original-notice fingerprints.
