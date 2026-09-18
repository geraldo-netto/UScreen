# Benchmark completion and foreground ownership (T485)

The decoder matrix, explicit decoder-plan runner and combined USB runner each
relaunched `com.uscreen/.MainActivity` after receiving a successful benchmark
result. That could replace an app the user selected immediately after the trial
completed. The benchmark Activity already calls `finish()`; a separate host
launch is unnecessary and cannot preserve the user's next foreground choice.

Removed all three completion launches. Android now finishes the Activity's own
task normally. Failed/backgrounded runs still abort their measurement; later USB
trials retain their existing foreground check before launch. No activity is
launched merely to restore focus after a measurement.

Three permanent tests in `scripts/tests/test_benchmark_completion.py` exercise
the actual driver completion paths with mocked Android calls. All three failed
before the change because a completion launch was issued; they pass afterwards
through the normal Cargo benchmark-tooling test. [Red/green logs and complexity
results](2026-09-18-benchmark-completion/) are retained. No device workload was
needed to validate removal of those host calls. T479's historical source
snapshots remain unchanged and identify the code used for its measurements.
