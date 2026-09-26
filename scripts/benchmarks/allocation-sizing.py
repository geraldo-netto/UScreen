#!/usr/bin/env python3
"""T587: deterministic Android buffer-demand model, not a runtime benchmark.

Counts requested payload-array bytes. Excludes headers, GC, allocator rounding,
codec storage and native memory. Resize overlap is two live array capacities,
not a process peak: a GC may retain additional unreachable arrays.
"""
import json

INITIAL = 512 * 1024
LIMIT = 8 * 1024 * 1024 + 1


def capacity(size, policy):
    if policy == 'exact_growth':
        return size
    if policy == 'capped_growth':
        return min(size + size // 2, LIMIT)
    return size + size // 2


def model(sizes, policy):
    retained = INITIAL
    requested = INITIAL
    allocations = 1
    overlap = INITIAL
    for size in sizes:
        if not 1 < size <= LIMIT:
            raise ValueError('packet size outside reader contract')
        if size > retained:
            new = capacity(size, policy)
            overlap = max(overlap, retained + new)
            retained = new
            requested += new
            allocations += 1
    return dict(allocations=allocations, requested_array_bytes=requested,
                final_capacity=retained, maximum_resize_overlap=overlap)


def workloads():
    return {
        'steady_64k': [64 * 1024] * 300,
        'six_mib_then_max_then_small': [6 * 1024 * 1024, LIMIT] + [64 * 1024] * 300,
        'maximum_then_small': [LIMIT] + [64 * 1024] * 300,
        'gradual_growth': list(range(INITIAL + 1, LIMIT, 64 * 1024)) + [LIMIT],
    }


def main():
    results = {}
    for name, sizes in workloads().items():
        results[name] = {policy: model(sizes, policy) for policy in
                         ('current_growth', 'capped_growth', 'exact_growth')}
    print(json.dumps(dict(kind='logical_allocation_model', initial=INITIAL,
                          maximum_packet=LIMIT, workloads=results), indent=2))


if __name__ == '__main__':
    main()
