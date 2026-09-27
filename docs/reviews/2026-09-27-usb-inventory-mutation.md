# USB inventory response bounds (T654)

The original tests coupled failed command status to empty output and supplied
only whitespace for oversized output. Parsing rejected those fixtures even when
status or size validation was removed.

Permanent `t654_inventory_status_and_size_are_independent_requirements` crosses
four exit statuses with valid inventory replies of 65,535/65,536/65,537/65,538/
131,072 bytes. Only successful commands at or below 65,536 bytes may publish a
known inventory. The test uses the existing fake command adapter and remains
in the normal suite. Production behavior is unchanged.

All three meaningful status/size mutants fail the strengthened test on Linux
and native Windows, with passing unmodified baselines. [Retained evidence](artifacts/2026-09-27-mutation-resume/t654/)
includes Linux's targeted campaign, selected Windows outcomes/baseline from the
complete 125-candidate rerun, logs, diffs and exact native source fingerprints.
Other campaign outcomes remain separate and are not credited to this regression.
