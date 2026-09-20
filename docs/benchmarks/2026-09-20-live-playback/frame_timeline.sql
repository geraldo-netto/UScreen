SELECT layer_name,jank_type,present_type,count(*) AS n FROM actual_frame_timeline_slice WHERE layer_name LIKE '%uscreen%' GROUP BY layer_name,jank_type,present_type;
