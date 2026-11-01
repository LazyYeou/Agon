import type {TelemetrySample} from './types/TelemetrySample';
import type {DriverState} from './types/DriverState';
import type {ChartKind} from './view';
export function distanceTrace(samples:TelemetrySample[]){
  let distance=0;
  return [...samples].sort((a,b)=>a.time_ms-b.time_ms).map((sample,i,list)=>{
    const previous=list[i-1];const dt=previous?(sample.time_ms-previous.time_ms)/1000:0;
    if(previous&&dt>0&&dt<=2&&previous.speed!=null&&sample.speed!=null)distance+=(previous.speed+sample.speed)/2/3.6*dt;
    return {...sample,distance,gap:dt>2};
  });
}
export function rolling(values:(number|null)[],n:number){return values.map((_,i)=>{const valid=values.slice(Math.max(0,i-n+1),i+1).filter((v):v is number=>v!=null);return valid.length?valid.reduce((a,b)=>a+b,0)/valid.length:null;});}
export function chartRows(driver:DriverState,kind:ChartKind,fuel:boolean,from:number,to:number):[number,number|null][] {
  const laps=driver.laps.filter(l=>l.number>=from&&l.number<=to);
  if(kind==='sectors')return [0,1,2].map(i=>{const rows=laps.map(l=>l.sectors[i]).filter((n):n is number=>n!=null);return [i+1,rows.length?rows.reduce((a,b)=>a+b,0)/rows.length:null];});
  if(kind==='stints')return driver.stints.map(s=>[s.start_lap,(s.end_lap??driver.lap)-s.start_lap+1]);
  return laps.map(l=>[kind==='degradation'?(l.tyre_age??l.number):l.number,
    kind==='position'?l.position:kind==='gap'||kind==='leader'?l.gap:
    kind==='speed'?null:l.duration==null?null:l.duration+(fuel?l.number*0.035:0)]);
}
export function insightDelta(a:DriverState,b:DriverState){
  const x=a.prediction?.pace[0],y=b.prediction?.pace[0];if(!x||!y||a.gap==null||b.gap==null)return 'Complete more clean laps to compare the pace trend.';
  const advantage=y.value-x.value;const gap=a.gap-b.gap;
  return advantage>0&&gap>0?`${a.driver.code} is estimated ${advantage.toFixed(2)} s/lap quicker; catch in ~${Math.ceil(gap/advantage)} laps. Basic estimate; traffic and pit stops may change this.`:`${a.driver.code} / ${b.driver.code}: pace difference ${advantage.toFixed(2)} s/lap. Forecast bands overlap; positions are uncertain.`;
}
