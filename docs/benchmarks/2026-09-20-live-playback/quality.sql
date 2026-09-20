SELECT name,idx,value,severity FROM stats WHERE severity IN ('error','data_loss') AND value>0;
