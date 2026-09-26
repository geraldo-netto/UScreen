# T626: CLI worker fixture drift

T613 had already set the shared libx264 default to one worker. The retained T373
CLI boundary test still expected automatic threading (no `-threads` option),
contradicting `common/src/encoding.rs`. The full suite exposed the mismatch.
The existing regression failed, then passed after its expected arguments gained
`-threads 1`. All codec/rate/quality assertions remain; production is unchanged.

[Red/green evidence](artifacts/2026-09-26-task-batch/t626/).
