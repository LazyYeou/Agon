import assert from 'node:assert/strict';
import { decode } from '@msgpack/msgpack';
export function percentile(values, fraction) {
  assert(values.length > 0 && fraction > 0 && fraction <= 1);
  return [...values].sort((a, b) => a - b)[Math.ceil(values.length * fraction) - 1];
}
if (process.argv.includes('--self-check')) {
  assert.equal(percentile([1, 100, 3, 4, 2], .95), 100);
  assert.equal(percentile([1, 2, 3], .5), 2);
  assert.throws(() => percentile([], .95));
  console.log('Load harness self-check passed');
} else {
  const option=(name,fallback)=>{const i=process.argv.indexOf(name);return i<0?fallback:Number(process.argv[i+1]);};
  const count=option('--clients',12), seconds=option('--seconds',5);
  assert(Number.isInteger(count)&&count>0&&count<=10000,'clients must be 1–10000');
  assert(Number.isInteger(seconds)&&seconds>0&&seconds<=60,'seconds must be 1–60');
  const url=process.env.AGON_WS_URL??'ws://127.0.0.1:3001/ws';
  const sockets=[], results=[];
  try {
    await Promise.all(Array.from({length:count},()=>new Promise((resolve,reject)=>{
      const socket=new WebSocket(url);sockets.push(socket);socket.binaryType='arraybuffer';
      const result={bytes:0,ack_ms:null,started:null};results.push(result);
      const timeout=setTimeout(()=>reject(Error('Timing handshake/control timed out')),10000);
      socket.onerror=()=>reject(Error('Connection failed; per-IP request/connection limits also apply to this harness'));
      socket.onmessage=event=>{
        const frame=decode(new Uint8Array(event.data));
        if(frame.kind==='snapshot'&&result.started===null){result.started=performance.now();socket.send(JSON.stringify({kind:'play',playing:true}));}
        else if(result.started!==null){result.bytes+=event.data.byteLength;
          if(frame.kind==='delta'&&frame.changes.playing===true&&result.ack_ms===null){result.ack_ms=performance.now()-result.started;clearTimeout(timeout);resolve();}
        }
      };
    })));
    const started=performance.now();const before=results.map(r=>r.bytes);
    await new Promise(resolve=>setTimeout(resolve,seconds*1000));
    const elapsed=(performance.now()-started)/1000;
    const rates=results.map((r,i)=>(r.bytes-before[i])/elapsed);
    console.log(JSON.stringify({clients:count,seconds:elapsed,control_ack_p95_ms:percentile(results.map(r=>r.ack_ms),.95),mean_bytes_per_second_per_client:rates.reduce((a,b)=>a+b,0)/count,max_bytes_per_second_per_client:Math.max(...rates),scope:'Local synthetic replay smoke. Control acknowledgement and network bytes only; not feed-to-screen, browser fps, production capacity or accuracy proof.'},null,2));
  } finally {for(const socket of sockets)socket.close();}
}
