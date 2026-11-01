import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import { homedir } from 'node:os';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
const root=fileURLToPath(new URL('../',import.meta.url));
const localCargo=path.join(homedir(),'.cargo','bin',process.platform==='win32'?'cargo.exe':'cargo');
const cargo=existsSync(localCargo)?localCargo:'cargo';
const children=[
  spawn(cargo,['run','-p','pitwall-api'],{cwd:root,stdio:'inherit'}),
  spawn(process.execPath,[path.join(root,'node_modules','vite','bin','vite.js'),'--host','127.0.0.1'],{cwd:path.join(root,'pitwall-web'),stdio:'inherit'}),
];
let stopping=false;
function stop(){if(stopping)return;stopping=true;for(const child of children){if(child.pid){if(process.platform==='win32')spawn('taskkill',['/PID',String(child.pid),'/T','/F'],{stdio:'ignore',windowsHide:true});else child.kill('SIGTERM');}}}
for(const event of ['SIGINT','SIGTERM','exit'])process.on(event,stop);
for(const child of children)child.on('exit',code=>{if(!stopping){stop();process.exitCode=code??1;}});
