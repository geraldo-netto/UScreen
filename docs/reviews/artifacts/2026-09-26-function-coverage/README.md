# T497 evidence

`combined.json.gz` records all 2,230 maintained functions/methods passing the
individual 80% executable-line threshold. One declined macOS function remains
explicitly unmeasured. `manifest.json.gz` is the complete source inventory.

Linux counters came from separate fresh LLVM target directories, default and
all-features builds. Windows counters came from native MSVC run
[36271915764](https://github.com/geraldo-netto/UScreen/actions/runs/36271915764)
at `f5cb33cea4e198b025bb0a3a1aa5fbb0dcf42cee`. `windows-sources.json.gz` attests
those source bytes; native tests/build logs also live in the adjacent T524
lifecycle evidence directory. Android XML is the fresh regular Robolectric
collection. C and script archives retain raw counters, source attestations,
test logs and their scoped reports.

The original pre-execution `linux-collection-manifest.json.gz` omitted C
headers from the combined inventory. The expanded manifest includes them;
`capture-native.tar.gz` contains the earlier pre-execution C manifest proving
that `pixel_damage.h` was already measured with identical bytes. Its four inline
functions each reach 100%. No production source changed for this inventory fix.

`*-red.log.gz` and corresponding green logs retain reporter regression evidence;
`reader-tests.log.gz` records the final 67 passing tests. `complexity.log.gz`
records 6,080 functions and no cyclomatic complexity above 9. These are bounded
runs, not claims of exhaustive fuzzing or physical GPU/USB acceptance.

To reproduce from the same source revision, decompress ordinary `.gz` files into
an evidence directory. Extract `capture-native.tar.gz` into `evidence/capture/`
and `scripts-native.tar.gz` into `evidence/scripts/`. Then run from the repo:

```sh
python scripts/coverage/report.py check \
  --manifest evidence/manifest.json \
  --lcov evidence/linux-default.lcov \
  --lcov evidence/linux-all-features.lcov \
  --lcov evidence/windows.lcov \
  --native-sources evidence/windows-sources.json \
  --jacoco evidence/android.xml \
  --gcov evidence/capture/native \
  --python-json evidence/scripts/python.json \
  --python-calls evidence/scripts/calls \
  --shell evidence/scripts/shell \
  --rust-platform linux --rust-platform windows \
  --prefix /backups/disk2/projects/blent \
  --output evidence/reproduced.json
```

Install `scripts/coverage/requirements.txt` in an isolated environment first.
The explicit prefix translates the original Linux collection root when replayed
from another checkout. Source hashes must match; a later source revision requires
new measurements. `SHA256SUMS` protects the archived evidence from accidental edits.
