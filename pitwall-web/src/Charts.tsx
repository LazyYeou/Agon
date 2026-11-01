import {lazy,Suspense,useMemo,useRef,useState} from 'react';
import type {Snapshot} from './types/Snapshot';
import type {DriverState} from './types/DriverState';
import type {ChartOption} from './EChart';
import {chartRows,insightDelta,rolling} from './analysis';
import {chartNames,num,tyreColor,readView,type ChartKind} from './view';
const EChart=lazy(()=>import('./EChart'));
export function ComparisonPlot({state,selected,kind,fuel=false,from=1,to=500,onReady}:{state:Snapshot;selected:number[];kind:ChartKind;fuel?:boolean;from?:number;to?:number;onReady?:(chart:import('echarts/core').ECharts)=>void}){
  const signature=Object.values(state.drivers).map(d=>d?.lap).join(',');
  const option=useMemo<ChartOption>(()=>{
    const drivers=selected.flatMap(n=>state.drivers[n]?[state.drivers[n]!]:[]);
    const series:Record<string,unknown>[]=[];
    drivers.forEach((d,index)=>{
      let data=chartRows(d,kind,fuel,from,to);
      if(kind==='gap'&&drivers[0]){const ref=new Map(chartRows(drivers[0],'leader',false,from,to));data=data.map(([lap,value])=>[lap,value==null||ref.get(lap)==null?null:value-ref.get(lap)!]);}
      if(kind==='sectors'&&drivers[0]){const ref=new Map(chartRows(drivers[0],'sectors',false,from,to));data=data.map(([sector,value])=>[sector,value==null||ref.get(sector)==null?null:value-ref.get(sector)!]);}
      const color=`#${d.driver.color}`;
      if(kind==='stints'){
        d.stints.forEach(st=>series.push({name:`${d.driver.code} S${st.number} ${st.compound}`,type:'bar',stack:`strategy`,barWidth:20,data:drivers.map(other=>other.driver.number===d.driver.number?((st.end_lap??d.lap)-st.start_lap+1):0),itemStyle:{color:tyreColor(st.compound)},label:{show:true,formatter:'{c}',color:'#101722'}}));
      }else{
        series.push({id:`${d.driver.number}-${kind}`,name:d.driver.code,type:kind==='sectors'?'bar':'line',showSymbol:kind==='position',symbolSize:4,connectNulls:false,data,lineStyle:{color,width:2,type:index%2?'dashed':'solid'},itemStyle:{color},animationDurationUpdate:250});
        if(kind==='pace'){const avg=rolling(data.map(([,v])=>v),3);series.push({name:`${d.driver.code} 3-lap mean`,type:'line',data:data.map(([lap],i)=>[lap,avg[i]]),showSymbol:false,lineStyle:{color,width:1,opacity:0.5,type:'dotted'}});}
        if(kind==='degradation'&&d.prediction?.pace.length){const age=d.laps.at(-1)?.tyre_age??d.lap;series.push({name:`${d.driver.code} forecast`,type:'line',data:d.prediction.pace.map((b,h)=>[age+h+1,b.value]),showSymbol:false,lineStyle:{color,type:'dashed'}});}
        if(kind==='accuracy'&&d.prediction?.pace.length){const p=d.prediction;series.push({name:`${d.driver.code} forecast`,type:'line',data:p.pace.map((b,h)=>[p.lap+h+1,b.value]),showSymbol:true,lineStyle:{color,type:'dashed'},markArea:{silent:true,itemStyle:{color,opacity:0.08},data:p.pace.map((b,h)=>[{xAxis:p.lap+h+0.6,yAxis:b.low},{xAxis:p.lap+h+1.4,yAxis:b.high}])}});}
      }
    });
    return {grid:{left:56,right:20,top:32,bottom:64},legend:{show:false},tooltip:{trigger:'axis',axisPointer:{type:'cross'}},
      xAxis:{type:kind==='stints'?'value':'value',name:kind==='degradation'?'Tyre age (laps)':kind==='sectors'?'Sector':'Lap',min:kind==='stints'?0:undefined,nameLocation:'middle',nameGap:32,axisLine:{lineStyle:{color:'#44566d'}},splitLine:{lineStyle:{color:'#223044'}}},
      yAxis:kind==='stints'?{type:'category',data:drivers.map(d=>d.driver.code)}:{type:'value',scale:true,inverse:kind==='position',name:kind==='position'?'Position':kind==='speed'?'km/h':kind==='gap'||kind==='leader'||kind==='sectors'?'Seconds Δ':'Seconds',splitLine:{lineStyle:{color:'#223044'}}},
      dataZoom:[{type:'inside'},{type:'slider',height:14,bottom:8,borderColor:'#29364a',fillerColor:'#75baff24'}],series};
  },[signature,selected.join(','),kind,fuel,from,to,state.session.key]);
  return <Suspense fallback={<p className="source-note">Loading chart renderer…</p>}><EChart option={option} onReady={onReady} label={`${chartNames[kind]}: ${kind==='accuracy'?'observed laps and forward forecast bands; history accumulates during playback':'compares completed lap data and labelled estimates'}. Drag or scroll to zoom.`}/></Suspense>;
}
export default function Charts({state,selected,setSelected,kind,setKind,onPin}:{state:Snapshot;selected:number[];setSelected:(n:number[])=>void;kind:ChartKind;setKind:(k:ChartKind)=>void;onPin:()=>void}){
  const [fuel,setFuel]=useState(false);const [from,setFrom]=useState(readView(location.search).from);const [to,setTo]=useState(readView(location.search).to);const [lastN,setLastN]=useState(5);const [share,setShare]=useState('');
  const [strategy,setStrategy]=useState<{undercut:{value:number;low:number;high:number}|null;overcut:{value:number;low:number;high:number}|null;assumptions:string[]}|null>(null);const [busy,setBusy]=useState(false);const [error,setError]=useState('');
  const chart=useRef<import('echarts/core').ECharts|null>(null);
  const drivers=Object.values(state.drivers).filter((d):d is DriverState=>!!d).sort((a,b)=>(a.position??99)-(b.position??99));
  const toggle=(n:number)=>{if(selected.includes(n)){if(selected.length>2)setSelected(selected.filter(v=>v!==n));}else if(selected.length<6)setSelected([...selected,n]);};
  const shareView=()=>{const p=new URLSearchParams({session:String(state.session.key),drivers:selected.join(','),chart:kind,from:String(from),to:String(to)});const url=`${location.origin}/?${p}`;history.replaceState(null,'',url);setShare(url);navigator.clipboard?.writeText(url).catch(()=>{});};
  const download=()=>{if(!chart.current)return;const link=document.createElement('a');link.download=`agon-${kind}-lap-${state.lap}.png`;link.href=chart.current.getDataURL({type:'png',pixelRatio:2,backgroundColor:'#151f2e'});link.click();};
  async function calculate(){setBusy(true);setError('');try{const response=await fetch(`/api/strategy?${new URLSearchParams({session:String(state.session.key),time_ms:String(state.clock_ms),a:String(selected[0]),b:String(selected[1])})}`);const data=await response.json();if(!response.ok)throw Error(data.error??'Calculation unavailable');setStrategy(data);}catch(e){setError(String(e));}finally{setBusy(false);}}
  return <section className="charts-panel"><div className="panel-heading"><div><h2>Driver comparison</h2><p>Select 2–6 drivers. New laps update automatically.</p></div><span className="pill">{selected.length} / 6 drivers</span></div>
    <div className="driver-picker">{drivers.map(d=><button key={d.driver.number} aria-pressed={selected.includes(d.driver.number)} disabled={!selected.includes(d.driver.number)&&selected.length>=6} onClick={()=>toggle(d.driver.number)} style={{'--team-color':`#${d.driver.color}`} as React.CSSProperties}>{d.driver.code}</button>)}</div>
    <div className="chart-toolbar"><label>Chart type<select aria-label="Chart type" value={kind} onChange={e=>setKind(e.target.value as ChartKind)}>{Object.entries(chartNames).map(([key,name])=><option key={key} value={key}>{name}</option>)}</select></label>
      <label>From lap<input type="number" min="1" max={Math.max(state.lap,1)} value={from} onChange={e=>setFrom(Math.max(1,Math.min(Number(e.target.value)||1,to)))}/></label><label>To lap<input type="number" min={from} max="500" value={to} onChange={e=>setTo(Math.max(from,Math.min(500,Number(e.target.value)||500)))}/></label>
      {kind==='sectors'?<label>Last N laps<input type="number" min="1" max="50" value={lastN} onChange={e=>setLastN(Math.min(50,Math.max(1,+e.target.value)))}/></label>:null}
      <label className="inline-check"><input type="checkbox" checked={fuel} onChange={e=>setFuel(e.target.checked)}/>Fuel correction (approx.)</label>
      <button onClick={onPin}>Pin chart</button><button onClick={download}>Export PNG</button><button onClick={shareView}>Share view</button>
    </div>
    {share?<label className="share-link">Share link<input aria-label="Share link" readOnly value={share} onFocus={e=>e.currentTarget.select()}/></label>:null}
    <ComparisonPlot state={state} selected={selected} kind={kind} fuel={fuel} from={kind==='sectors'?Math.max(from,state.lap-lastN+1):from} to={to} onReady={c=>{chart.current=c;}}/>
    <div className="chart-legend">{selected.flatMap((n,i)=>state.drivers[n]?[<span key={n}><i style={{background:`#${state.drivers[n]!.driver.color}`,borderTop:i%2?'2px dashed white':undefined}}/>{state.drivers[n]!.driver.code}</span>]:[])}</div>
    <p className="source-note">{kind==='speed'?'Speed-trap data appears when the source supplies a measured trap value.':kind==='accuracy'?'Solid = actual; dashed = forecast; shaded rectangles = p10–p90. Forecasts start after issue lap; completed backtest scores are separate.':kind==='stints'?'Completed/current stints only. Future strategy requires a calibrated pit model.':'Clean-lap trends are approximate. Pit/safety-car laps remain visible in the driver history and are excluded from the pace fit.'}</p>
    <section className="strategy-panel"><div className="panel-heading"><div><h3>Undercut / overcut</h3><p>{selected.slice(0,2).map(n=>state.drivers[n]?.driver.code).join(' vs ')}</p></div><button disabled={busy} onClick={calculate}>{busy?'Calculating…':'Calculate strategy'}</button></div>
      {selected.length>=2&&state.drivers[selected[0]]&&state.drivers[selected[1]]?<p>{insightDelta(state.drivers[selected[0]]!,state.drivers[selected[1]]!)}</p>:null}
      {error?<p role="alert">{error}</p>:null}{strategy?<><div className="strategy-results">{(['undercut','overcut'] as const).map(key=><p key={key}>{key==='undercut'?'Undercut':'Overcut'} advantage<strong>{strategy[key]?`${num(strategy[key]!.value,2)} s`:'Unavailable'}</strong><small>{strategy[key]?`${num(strategy[key]!.low,2)} to ${num(strategy[key]!.high,2)} s · basic estimate`:'Needs valid pace and pit-loss data'}</small></p>)}</div>{strategy.assumptions.map((a,i)=><p className="source-note" key={i}>{a}</p>)}</>:<p className="source-note">Recompute at the displayed replay time. Results include assumptions and uncertainty.</p>}
    </section>
  </section>;
}
