SELECT frame_number,name,ts,dur,layer_name FROM frame_slice WHERE layer_name LIKE '%uscreen%' AND name IN ('Dequeue','Queue','Latch','PresentFenceSignaled') ORDER BY ts;
