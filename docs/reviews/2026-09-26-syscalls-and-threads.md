# Syscall and thread follow-up

The T596 traces support investigating readback/copies and thread wakeups, not
replacing APIs based on summed blocking time. No runtime settings or production
code changed in this follow-up. Camera capture remains outside the workload.

## futex, readiness and reads

The moving host trace contains 119,013 futex calls in ten seconds, with 316.64
thread-seconds of elapsed time. This is an aggregate across the selected host
processes and their traced descendants. It neither attributes calls to one
application mutex nor separates productive wakeups from unnecessary wakeups.
Futexes implement both lock contention and condition-variable/runtime parking;
uncontended locking can stay in userspace. See [futex(7)](https://man7.org/linux/man-pages/man7/futex.7.html).
Next evidence is per-process/thread operation and wakeup attribution, runnable
delay, and critical-path lock wait/hold time where demonstrated. Spinning in
place of sleeping could increase CPU and battery cost.

Long `epoll_wait`, `poll` or blocking `read` calls can simply mean that no work is
available. Reduce unnecessary wakeups, tiny reads or copies only after identifying
their source. The [earlier T405 readiness work](../benchmarks/2026-09-17-readiness.md)
already replaced empty-FIFO polling: its isolated single-reader fixture changed
from 487 reads/s to one, and CPU from 2.485 to 0.191 ms. Those are historical
fixture results, not a new whole-application improvement.

`poll` inspects the supplied descriptor set; `epoll` maintains registered
interests and a ready list, useful when monitoring many mostly idle descriptors.
It is not a blanket O(1) whole-program guarantee: registration and handling ready
events still cost work. See [epoll(7)](https://man7.org/linux/man-pages/man7/epoll.7.html)
and [poll(2)](https://man7.org/linux/man-pages/man2/poll.2.html).
Our native capture wait monitors one or two descriptors
(`host/evdi/capture.c:poll_capture_events`); the GPU event loop monitors one
(`host/gpu/events.c:gpu_events_wait`). There is no demonstrated descriptor-scan
bottleneck to justify adding epoll registration/lifecycle here. Keep existing
epoll users and these small poll sets. T593's deadline/phase investigation is
distinct from replacing the event notification API.

## ioctl and newer I/O APIs

`ioctl` invokes a device-specific driver contract, not an obsolete generic
read/write layer. Changing its entry mechanism does not remove the work the
driver performs. [ioctl(2)](https://man7.org/linux/man-pages/man2/ioctl.2.html)
describes that device-specific interface; `IORING_OP_URING_CMD` offers private
asynchronous operations for supporting files/drivers, not a universal translation
of existing ioctl requests. See [io_uring_enter(2)](https://man7.org/linux/man-pages/man2/io_uring_enter.2.html).
No compatible alternative EVDI readback command has been established here.

The host CPU profile actually samples EVDI readback, `_copy_to_user`, DRM cache
flushing and conversion. T599 therefore investigates avoiding redundant readback
under an admitted same-GPU consumer, preserving display ownership and fallback.
This can remove work rather than merely change its submission mechanism; benefit
remains unmeasured. It depends on T593, and cross-GPU admission remains T578.

The [T405 io_uring experiment](../benchmarks/2026-09-17-io-uring.md) already compared
ordinary WRITE/SEND against a small readiness loop. At 60 FPS it found no
consistent CPU or tail win. One pipe changed from 76.31 to 81.40 ms process CPU
and 0.525 to 0.715 ms write p99; other cases improved. These measurements do not
evaluate all ring features or reproduce Tokio, and do not establish a production
reason to migrate. Ordinary ring I/O does not automatically remove payload copies.

## Thread budgets: T600

The [read-only inventory](artifacts/2026-09-26-thread-inventory.json) records a
later live snapshot, separate from the T596 recording:

| Process | Threads | Interpretation |
| --- | ---: | --- |
| Blent daemon | 36 | 33 named `tokio-rt-worker`; names alone do not separate scheduler workers from blocking workers. |
| EVDI helper | 31 | Auto pool uses allowed CPUs minus two, including the caller, plus the writer. |
| FFmpeg | 43 | Not all threads can be attributed to libx264 from this count. |
| ADB | 6 | Transport-owned threads, not Blent runtime settings. |

`host/src/linux_main.rs` uses the default `#[tokio::main]` runtime.
`host/evdi/evdi_helper.c:conv_pool_init` supports explicit conversion capacity;
`host/evdi/conversion.c:job_count` limits work to roughly one participant per
262,144 dirty source pixels and signals only selected workers. At 1280x800,
full-frame scale-1 conversion needs at most four participants, including the
caller. Spare sleeping threads are not per-frame work. Reducing them may save
stack reservations/kernel resources, but resident-memory and CPU gains need
measurement. Fewer active workers can also lengthen conversion/encoding or
delay unrelated tasks.

T600 should compare runtime, conversion and encoder budgets separately, using
the existing single tablet, repeated matched scenes and one changed setting at
a time. Record process/thread CPU, context switches, runnable delay, resident
memory, render-ACK p95/p99 and dropped frames. Do not infer battery savings from
thread count. Retain configurable capacity and cancellation/ownership regressions
before any production policy change. Android codec/Binder threads include
framework-owned work; merging Blent's receiver/output/callback responsibilities
could delay ACKs or create blocking dependencies and is not justified here.

## Dependency order

- T598 supplies better attribution: optimized shell-profileable full Android
  client, better host stacks, then scoped syscall/wakeup evidence for T600.
- T593 resolves GPU source/capture phase behavior before T599 changes capture
  readback ownership. T599 remains a stronger copy-reduction candidate than a
  wholesale ioctl replacement.
- T600 measures thread budgets; it makes no promise of speedup or default change.

Documentation and evidence only; no production tests were rerun for this review.
