import {it,expect} from 'vitest';
import { distanceTrace, rolling, chartRows } from './analysis';
import type { DriverState } from './types/DriverState';
it('integrates speed in km/h against elapsed seconds and preserves missing samples',()=>{
  const rows=distanceTrace([{time_ms:0,speed:36,rpm:null,gear:null,throttle:null,brake:null,drs:null,x:null,y:null},{time_ms:1000,speed:72,rpm:null,gear:null,throttle:null,brake:null,drs:null,x:null,y:null}]);
  expect(rows[1].distance).toBe(15);
  expect(distanceTrace([{...rows[0],speed:null}])[0].distance).toBe(0);
});
it('rolling pace ignores missing durations instead of turning them into zero',()=>{
  expect(rolling([90,null,92,94],3)).toEqual([90,90,91,93]);
});
it('maps lap, position and sector values from the selected causal history',()=>{
  const d={laps:[{number:1,duration:90,position:2,gap:1.5,sectors:[30,31,29],tyre_age:1},{number:2,duration:null,position:1,gap:0,sectors:[null,null,null],tyre_age:2}]} as DriverState;
  expect(chartRows(d,'pace',false,1,1)).toEqual([[1,90]]);
  expect(chartRows(d,'position',false,1,2)).toEqual([[1,2],[2,1]]);
  expect(chartRows(d,'sectors',false,1,2)).toEqual([[1,30],[2,31],[3,29]]);
});
