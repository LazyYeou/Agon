import {lazy,Suspense,useEffect,useMemo,useState} from 'react';
import type {Snapshot} from './types/Snapshot';
import type {DriverState} from './types/DriverState';
import type {TelemetrySample} from './types/TelemetrySample';
import Modal from './Modal';
import type {ChartOption} from './EChart';
import {distanceTrace} from './analysis';
import {getLocal,setLocal,lapTime,num,tyreColor} from './view';
const EChart=lazy(()=>import('./EChart'));
export default function TelemetryPanel({state,number,samples,onClose}:{state:Snapshot;number:number;samples:TelemetrySample[];onClose:()=>void}){
  const d=state.drivers[number]!;const [expert,setExpert]=useState(getLocal('agon-expert')==='true');const [lap,setLap]=useState(d.laps.at(-1)?.number??1);const [reference,setReference]=useState(number);const [referenceLap,setReferenceLap]=useState(0);
  const [trace,setTrace]=useState<TelemetrySample[]>([]);const [overlay,setOverlay]=useState<TelemetrySample[]>([]);const [loading,setLoading]=useState(false);const [error,setError]=useState('');const [hover,setHover]=useState(0);
  const driverList=Object.values(state.drivers).filter((v):v is DriverState=>!!v);
  const lapData=d.laps.find(l=>l.number===lap);const refDriver=state.drivers[reference];
  const refData=referenceLap?refDriver?.laps.find(l=>l.number===referenceLap):refDriver?.laps.reduce<(typeof d.laps)[number]|null>((best,l)=>l.duration!=null&&(!best||best.duration==null||l.duration<best.duration)?l:best,null);
  useEffect(()=>{
    const controller=new AbortController();setLoading(true);setError('');setTrace([]);setOverlay([]);
    async function fetchLap(driver:number,item:typeof lapData){if(!item?.duration)return [];const end=item.time_ms;const from=Math.max(state.session.start_ms,end-Math.min(180,item.duration)*1000);const response=await fetch(`/api/telemetry?${new URLSearchParams({session:String(state.session.key),driver:String(driver),from:String(Math.floor(from)),to:String(end)})}`,{signal:controller.signal});const value=await response.json();if(!response.ok)throw Error(value.error??'Telemetry unavailable');return value as TelemetrySample[];}
    Promise.all([fetchLap(number,lapData),fetchLap(reference,refData??undefined)]).then(([a,b])=>{setTrace(a);setOverlay(b);}).catch(e=>{if(!controller.signal.aborted)setError(String(e));}).finally(()=>{if(!controller.signal.aborted)setLoading(false);});
    return()=>controller.abort();
  },[number,lap,reference,refData?.number,state.session.key]);
  const rows=useMemo(()=>distanceTrace(trace),[trace]);const refRows=useMemo(()=>distanceTrace(overlay),[overlay]);
  const current=samples.at(-1)??trace.at(-1);const p=d.prediction;const stint=d.stints.at(-1);const maxDistance=rows.at(-1)?.distance??0;
  const option=useMemo<ChartOption>(()=>{
    const metrics=[['speed','Speed · km/h'],['throttle','Throttle · %'],['brake','Brake'],['gear','Gear'],['rpm','RPM'],['delta','Delta · s']] as const;
    return {tooltip:{trigger:'axis',axisPointer:{type:'cross'}},axisPointer:{link:[{xAxisIndex:'all'}]},
      grid:metrics.map((_,i)=>({left:66,right:22,top:18+i*91,height:62})),
      xAxis:metrics.map((_,i)=>({type:'value',gridIndex:i,min:0,max:maxDistance||1,axisLabel:{show:i===5},name:i===5?'Approximate distance (m)':'',nameLocation:'middle',nameGap:24,splitLine:{show:false},axisLine:{lineStyle:{color:'#3b4a60'}}})),
      yAxis:metrics.map(([_,label],i)=>({type:'value',gridIndex:i,name:label,nameLocation:'end',nameTextStyle:{align:'left',fontSize:10},scale:true,splitLine:{lineStyle:{color:'#223044'}}})),
      series:metrics.flatMap(([key],i)=>{
        if(key==='delta')return [{name:'Delta vs reference',type:'line',xAxisIndex:i,yAxisIndex:i,showSymbol:false,data:rows.map(row=>{const reference=refRows.find(r=>r.distance>=row.distance);return [row.distance,reference?(row.time_ms-(rows[0]?.time_ms??0)-(reference.time_ms-(refRows[0]?.time_ms??0)))/1000:null];}),lineStyle:{color:'#ecc77d',width:1.5}}];
        return [{name:`${d.driver.code} ${key}`,type:'line',xAxisIndex:i,yAxisIndex:i,showSymbol:false,smooth:0.15,connectNulls:false,data:rows.map(r=>[r.distance,r.gap?null:key==='brake'?(r.brake==null?null:r.brake?100:0):r[key]]),lineStyle:{color:`#${d.driver.color}`,width:1.6}},
          {name:`Reference ${key}`,type:'line',xAxisIndex:i,yAxisIndex:i,showSymbol:false,smooth:0.15,data:refRows.map(r=>[r.distance,r.gap?null:key==='brake'?(r.brake==null?null:r.brake?100:0):r[key]]),lineStyle:{color:'#b8a0ee',type:'dashed',width:1.2,opacity:0.75}}];
      }),
    };
  },[trace,overlay,maxDistance]);
  const historyOption:ChartOption={grid:{left:55,right:20,top:20,bottom:35},tooltip:{trigger:'axis'},xAxis:{type:'value',name:'Lap'},yAxis:{type:'value',scale:true,name:'Seconds'},series:[{type:'line',showSymbol:true,data:d.laps.map(l=>[l.number,l.duration]),lineStyle:{color:`#${d.driver.color}`},markLine:{symbol:'none',data:d.pits.map(p=>({xAxis:p.lap,label:{formatter:'Pit'}}))},markArea:{silent:true,data:d.stints.map(st=>[{xAxis:st.start_lap,itemStyle:{color:tyreColor(st.compound),opacity:0.08}},{xAxis:st.end_lap??d.lap}])}}]};
  const positionOption:ChartOption={grid:{left:50,right:20,top:15,bottom:30},tooltip:{trigger:'axis'},xAxis:{type:'value',name:'Lap'},yAxis:{type:'value',inverse:true,min:1,name:'Position'},series:[{type:'line',step:'end',data:d.laps.map(l=>[l.number,l.position]),lineStyle:{color:`#${d.driver.color}`}}]};
  const pointIndex=maxDistance?Math.round(hover/maxDistance*(state.circuit.outline.length-1)):0;const point=state.circuit.outline[Math.max(0,Math.min(pointIndex,state.circuit.outline.length-1))];
  return <Modal drawer label={`${d.driver.code} telemetry`} onClose={onClose}>
    <div className="drawer-header"><div><span className="driver-overline">Car {number} · {d.driver.team}</span><h2><i style={{background:`#${d.driver.color}`}}/>{d.driver.code}<span>Telemetry</span></h2></div><button aria-label="Close telemetry" onClick={onClose}>✕</button></div>
    <div className="mode-toggle"><button aria-pressed={!expert} onClick={()=>{setExpert(false);setLocal('agon-expert','false');}}>Beginner</button><button aria-pressed={expert} onClick={()=>{setExpert(true);setLocal('agon-expert','true');}}>Expert</button></div>
    <div className="gauges">{[['Speed',current?.speed,'km/h'],['Gear',current?.gear,''],['RPM',current?.rpm,'rpm'],['Throttle',current?.throttle,'%']].map(([name,value,unit])=><div key={name}><span>{name}</span><strong>{num(value as number|null|undefined,0)}<small>{unit}</small></strong></div>)}<div><span>Brake</span><strong>{current?.brake==null?'—':current.brake?'On':'Off'}</strong></div><div><span>DRS</span><strong>{current?.drs==null?'Unknown':current.drs?'Open':'Closed'}</strong></div></div>
    <div className="stint-overview"><b style={{color:tyreColor(stint?.compound??'')}}>{stint?.compound??'Unknown tyre'} · S{stint?.number??'—'}</b><span>Age {d.laps.at(-1)?.tyre_age??'—'} laps</span><span>Deg {p?.degradation?`${num(p.degradation.low,2)} to ${num(p.degradation.high,2)} s/lap`:'unavailable'}</span><span>Cliff estimate {p?.laps_to_cliff?`${num(p.laps_to_cliff.low,0)}–${num(p.laps_to_cliff.high,0)} laps`:'unavailable'}</span><span>Pit {p?.pit_window?`L${p.pit_window.from_lap}–${p.pit_window.to_lap} · uncalibrated`:'window unavailable'}</span></div>
    <p className="insight">P{d.position??'—'} · Gap ahead {num(d.interval)} s · {p?.insight??'Complete clean laps for a plain-language insight.'}</p>
    <div className="panel-heading"><h3>Lap telemetry</h3><span className="pill">Approximate telemetry</span></div><p className="source-note">~3.7 Hz source; distance is integrated from speed. Smoothed traces and corner estimates do not imply engineering precision. Gaps stay visible.</p>
    <div className="trace-controls"><label>Driver lap<select value={lap} onChange={e=>setLap(+e.target.value)}>{d.laps.map(l=><option key={l.number} value={l.number}>L{l.number} · {lapTime(l.duration)}</option>)}</select></label><label>Reference driver<select value={reference} onChange={e=>{setReference(+e.target.value);setReferenceLap(0);}}>{driverList.map(v=><option key={v.driver.number} value={v.driver.number}>{v.driver.code}</option>)}</select></label><label>Reference lap<select value={referenceLap} onChange={e=>setReferenceLap(+e.target.value)}><option value={0}>Best completed lap</option>{refDriver?.laps.map(l=><option key={l.number} value={l.number}>L{l.number} · {lapTime(l.duration)}</option>)}</select></label></div>
    {loading?<p role="status">Loading selected lap traces…</p>:null}{error?<p role="alert" className="error-banner">{error}</p>:null}
    {!loading&&!trace.length?<div className="empty">No telemetry samples for this lap. Select another completed lap.</div>:<><Suspense fallback={<p className="source-note">Loading trace chart…</p>}><EChart option={option} height={585} label="Stacked speed, throttle, brake, gear, RPM and delta-time traces against approximate lap distance. Hover shares a crosshair." onHover={setHover}/></Suspense><div className="trace-map"><svg viewBox="0 0 1000 600" aria-label="Approximate hover location on circuit"><polyline points={state.circuit.outline.map(p=>p.join(',')).join(' ')} fill="none" stroke="#71859e" strokeWidth="10"/>{point?<circle cx={point[0]} cy={point[1]} r="22" fill={`#${d.driver.color}`}/>:null}</svg><span>Crosshair distance {num(hover,0)} m · approximate position</span></div></>}
    {expert?<>
      <h3>Corner breakdown</h3><p className="source-note">Approximate braking onset, apex speed and exit. Coordinates identify corners; metres are calculated from speed. Reference is the selected lap.</p>
      {state.circuit.corners.length&&rows.some(r=>r.x!=null)?<div className="table-scroll"><table className="detail-table"><thead><tr><th>Corner</th><th>Brake before apex</th><th>Apex speed</th><th>Exit to full throttle</th><th>Time Δ</th></tr></thead><tbody>{state.circuit.corners.map(([name,x,y])=>{
        const closest=(data:typeof rows)=>data.reduce((a,b)=>Math.hypot((b.x??Infinity)-x,(b.y??Infinity)-y)<Math.hypot((a.x??Infinity)-x,(a.y??Infinity)-y)?b:a,data[0]);const apex=closest(rows);const ref=refRows.length?closest(refRows):null;
        const before=rows.filter(r=>r.distance<=apex.distance&&apex.distance-r.distance<300);const brake=before.find(r=>r.brake);const exit=rows.find(r=>r.distance>=apex.distance&&r.throttle!=null&&r.throttle>=99);
        return <tr key={name}><th>{name}</th><td>{brake?`${num(apex.distance-brake.distance,0)} m`:'—'}</td><td>{num(apex.speed,0)} km/h {ref?.speed!=null&&apex.speed!=null?<small>Δ {num(apex.speed-ref.speed,0)}</small>:null}</td><td>{exit?`${num((exit.time_ms-apex.time_ms)/1000,2)} s`:'—'}</td><td>{ref?`${num((apex.time_ms-rows[0].time_ms-(ref.time_ms-refRows[0].time_ms))/1000,2)} s`:'—'}</td></tr>;
      })}</tbody></table></div>:<p className="empty">Corner coordinates or lap position samples unavailable for this configuration.</p>}
      <h3>Race history</h3><Suspense fallback={<p>Loading race history chart…</p>}><EChart option={historyOption} label="Completed lap times; shaded compound stints and marked pit stops."/></Suspense>
      <table className="detail-table"><caption>Tyre history</caption><thead><tr><th>Stint</th><th>Compound</th><th>Laps</th><th>Mean pace</th><th>Deg</th></tr></thead><tbody>{d.stints.map(st=>{const laps=d.laps.filter(l=>l.number>=st.start_lap&&l.number<=(st.end_lap??d.lap)&&l.duration!=null);return <tr key={st.number}><th>{st.number}</th><td style={{color:tyreColor(st.compound)}}>{st.compound}</td><td>{st.start_lap}–{st.end_lap??d.lap}</td><td>{laps.length?lapTime(laps.reduce((a,l)=>a+l.duration!,0)/laps.length):'—'}</td><td>{st.number===stint?.number&&p?.degradation?`${num(p.degradation.value,3)} ± ${num((p.degradation.high-p.degradation.low)/2,3)}`:'Unavailable'}</td></tr>;})}</tbody></table>
      <h3>Position by lap</h3><Suspense fallback={<p>Loading position chart…</p>}><EChart option={positionOption} height={200} label="Recorded position at each completed lap; lower position is nearer the lead."/></Suspense>
      <h3>Sector bests</h3><div className="gauges">{[0,1,2].map(i=><div key={i}><span>Sector {i+1}</span><strong>{num(Math.min(...d.laps.flatMap(l=>l.sectors[i]!=null?[l.sectors[i]!]:[])),3)} s</strong></div>)}</div>
      <h3>Driver race control</h3>{state.messages.filter(m=>m.driver===number).length?state.messages.filter(m=>m.driver===number).map((m,i)=><p key={i}>{m.message}</p>):<p className="source-note">No driver-specific messages at this replay time.</p>}
    </>:null}
  </Modal>;
}
