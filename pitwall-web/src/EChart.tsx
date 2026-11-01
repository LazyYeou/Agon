import {useEffect,useRef} from 'react';
import * as echarts from 'echarts/core';
import {LineChart,BarChart,ScatterChart,CustomChart} from 'echarts/charts';
import {GridComponent,TooltipComponent,LegendComponent,DataZoomComponent,MarkLineComponent,MarkAreaComponent,AriaComponent} from 'echarts/components';
import {CanvasRenderer} from 'echarts/renderers';
echarts.use([LineChart,BarChart,ScatterChart,CustomChart,GridComponent,TooltipComponent,LegendComponent,DataZoomComponent,MarkLineComponent,MarkAreaComponent,AriaComponent,CanvasRenderer]);
export type ChartOption=echarts.EChartsCoreOption;
export default function EChart({option,label,height=280,onReady,onHover}:{option:ChartOption;label:string;height?:number;onReady?:(chart:echarts.ECharts)=>void;onHover?:(index:number)=>void}){
  const container=useRef<HTMLDivElement>(null);const chart=useRef<echarts.ECharts|null>(null);const hover=useRef(onHover);hover.current=onHover;
  useEffect(()=>{
    const instance=echarts.init(container.current!,null,{renderer:'canvas'});chart.current=instance;instance.group='agon-crosshair';echarts.connect('agon-crosshair');onReady?.(instance);
    const resize=new ResizeObserver(()=>instance.resize());resize.observe(container.current!);
    instance.on('updateAxisPointer',(event:unknown)=>{const e=event as {dataIndex?:number;axesInfo?:{value:number}[]};if(e.axesInfo?.[0])hover.current?.(e.axesInfo[0].value);});
    return()=>{resize.disconnect();instance.dispose();chart.current=null;};
  },[]);
  useEffect(()=>{chart.current?.setOption({backgroundColor:'transparent',textStyle:{color:'#a5b5c9',fontFamily:'Segoe UI'},aria:{enabled:true},animationDuration:250,animationDurationUpdate:250,...option},{replaceMerge:['series'],lazyUpdate:true});},[option]);
  return <div ref={container} role="img" aria-label={label} title={label} style={{height,width:'100%'}}/>;
}
