# TODO

| id | status | severity | effort | description |
|---|---|---|---|---|
| T220 | open | low | small | `Makefile:build` invokes Cargo without GNU Make recursive recipe marking; `make -j2 build` closes advertised jobserver descriptors and Rust reports an inaccessible jobserver. Preserve parallelism handoff and dry-run behavior. Add permanent normal-suite test proving Cargo receives usable pipe/FIFO jobserver access and `make -n` never executes it; reproduce descriptor failure before fixing. |
