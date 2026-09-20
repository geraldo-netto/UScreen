SELECT layer_name,name,count(*) AS n FROM frame_slice WHERE layer_name LIKE '%uscreen%' GROUP BY layer_name,name;
