# T697 capture capacity preparation

`common::capture_capacity` separates requested settings, a bounded resource plan,
and effective capacity reported after allocation/worker startup. It reuses the
configuration's 1/2/4/8 MiB choices (default 1) and Auto/1..128 conversion limit.
Auto reserves two available CPUs, clamps to 1..128 and includes the calling
worker, matching Linux's capacity policy. Native adapters must supply a nonzero
available-CPU count; unknown evidence is an error. Dirty work uses at most the
started capacity; an idle frame uses no conversion workers.

The Windows `OwnedBuffer` adapter commits owned, zeroed, writable memory using
[VirtualAlloc](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-virtualalloc).
Reads and writes reject overflow and out-of-bounds ranges. Drop releases only
that original reservation using
[VirtualFree](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-virtualfree).
It implements the portable buffer interface; no Linux FIFO control or kernel
pipe-size claim is involved. Effective buffer bytes and successfully started
workers are reported separately from requested values, including reduced
capacity. Invalid evidence cannot produce a successful report.

Permanent shared tests cover defaults, Auto/manual decisions, unknown CPU counts,
all supported sizes, invalid values, overflow boundaries, reduced resources and
idle/dirty work. Native tests allocate each supported size, verify zeroing and
boundary writes, fuzz 512 bounded offset/length pairs, query release of owned
memory and report capacity from successfully started/joined OS-thread fixtures.
They do not establish converter correctness or performance.

This buffer is a transport building block, not a frame queue or a Windows
conversion pool. Production capture wiring, native CPU-affinity discovery and
persistent conversion workers remain T528/T529 integration work. The GUI and
capability flags stay unavailable until those adapters use these settings and
pass native acceptance. Larger buffers trade memory and possible queue latency
for burst capacity; more workers trade CPU use for conversion concurrency.
Neither faster conversion, lower latency nor zero-copy behavior is claimed.
