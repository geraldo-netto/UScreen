SELECT cpu,sum(dur)/1e6 AS idle_ms FROM sched WHERE utid=0 AND dur>0 GROUP BY cpu;
