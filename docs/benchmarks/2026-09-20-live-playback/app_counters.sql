SELECT ct.name,count(*) AS n,min(c.value) AS min,max(c.value) AS max FROM counter c JOIN process_counter_track ct ON c.track_id=ct.id JOIN process p USING(upid) WHERE p.pid=22610 GROUP BY ct.name;
