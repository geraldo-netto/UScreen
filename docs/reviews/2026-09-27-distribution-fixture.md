# Distribution fixture library ownership (T642)

[Run 36274495310](https://github.com/geraldo-netto/UScreen/actions/runs/36274495310)
failed T102 and its T302 version-bump repetition because the compiler selected
installed libevdi before the fixture's `LIBRARY_PATH`. The system library correctly
lacks the fixture-only `blent_distribution_probe` symbol.

The permanent distribution fixture now supplies a competing compiler library
through `-B`, reproducing that failure without depending on the machine's packages.
It then explicitly prioritizes the owned fixture library with `-L`. The same
fixture fails without that priority and passes with it; packaging, symlink,
notice and extracted-helper loader assertions remain unchanged. Quoted paths,
including the existing apostrophe/space case, remain exercised. No production
build or packaging behavior changed.
