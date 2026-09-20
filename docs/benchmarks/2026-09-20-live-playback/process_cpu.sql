SELECT p.name,round(sum(s.dur)/1e6,3) AS cpu_ms FROM sched s JOIN thread t USING(utid) JOIN process p USING(upid) WHERE s.dur>0 GROUP BY p.upid ORDER BY cpu_ms DESC LIMIT 12;
