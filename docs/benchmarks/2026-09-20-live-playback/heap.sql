SELECT c.ts,c.value FROM counter c JOIN process_counter_track ct ON c.track_id=ct.id JOIN process p USING(upid) WHERE p.pid=22610 AND ct.name='Heap size (KB)' ORDER BY ts;
