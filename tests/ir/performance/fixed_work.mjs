// Equal guest work, warmed code, fresh VM for each policy. No timer-limited loops.
import assert from "node:assert/strict";
import fs from "node:fs";
import { finish_halted_timing } from "./timing.mjs";
import {V86} from "../../../build/libv86.mjs";
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
const repetitions=Number(process.env.IR_COMPARE_RUNS||3);
assert(Number.isInteger(repetitions)&&repetitions>=3);
const PC=0x100000,PEER=0x102000,DATA=0x110000;
const suite=process.env.IR_FIXED_SUITE||"core";
assert(["core","vector"].includes(suite),"IR_FIXED_SUITE must be core or vector");
const workloads=suite==="vector"?[
 {name:"sse_packed_double",code:[0x66,0x0F,0x58,0xC1,0x49,0x75,0xF9,0xF4],iterations:2000000,per:3,source:[0,0x3FF00000,0,0x3FF00000]},
 {name:"sse_scalar_single",code:[0xF3,0x0F,0x58,0xC1,0x49,0x75,0xF9,0xF4],iterations:2000000,per:3,destination:[0,0x7F812345,0xDEADBEEF,0x81234567]},
 {name:"sse_scalar_double",code:[0xF2,0x0F,0x58,0xC1,0x49,0x75,0xF9,0xF4],iterations:2000000,per:3,source:[0,0x3FF00000,0x7FF12345,0xDEADBEEF],destination:[0,0,0x7FF12345,0xDEADBEEF]},
 {name:"xmm_store",code:[0x0F,0x11,0x06,0x49,0x75,0xFA,0xF4],iterations:1000000,per:3,destination:[0x12345678,0x9ABCDEF0,0x3456789A,0xBCDEF012]},
 {name:"xmm_high_store",code:[0x0F,0x17,0x06,0x49,0x75,0xFA,0xF4],iterations:1000000,per:3,destination:[0x12345678,0x9ABCDEF0,0x3456789A,0xBCDEF012]},
 {name:"xmm_masked_store",code:[0x66,0x0F,0xF7,0xC1,0x49,0x75,0xF9,0xF4],iterations:1000000,per:3,edi:DATA,source:[0x80808080,0x80808080,0x80808080,0x80808080],destination:[0x12345678,0x9ABCDEF0,0x3456789A,0xBCDEF012]},
]:[
 {name:"integer",code:[0x01,0xD8,0x31,0xD0,0x43,0x49,0x75,0xF8,0xF4],iterations:5000000,per:5},
 {name:"ram_rmw",code:[0xFF,0x06,0x8B,0x06,0x01,0xC3,0x49,0x75,0xF7,0xF4],iterations:3000000,per:5},
 {name:"indirect_regions",code:[0x40,0xFF,0xE2],peer:[0x49,0x74,0x02,0xFF,0xE3,0xF4],iterations:3000000,per:5},
 {name:"sse_register",code:[0x0F,0x58,0xC1,0x49,0x75,0xFA,0xF4],iterations:2000000,per:3},
];
const [wasmPath="build/v86-ir-runtime.wasm",baselineWasm,...extra]=process.argv.slice(2);
assert.equal(extra.length,0,"usage: fixed_work.mjs [current.wasm] [baseline.wasm]");
const variants=[{label:"current",wasm:wasmPath}];
if(baselineWasm) variants.push({label:"baseline",wasm:baselineWasm});
const scale=Number(process.env.IR_FIXED_SCALE||1);
assert(Number.isInteger(scale)&&scale>=1&&scale<=50,"IR_FIXED_SCALE must be an integer 1..50");
for(const work of workloads) work.iterations*=scale;
const results=[];
const arms=variants.flatMap(v=>["ir","legacy"].map(backend=>({...v,backend})));
for(const work of workloads) for(let round=0;round<repetitions;round++) for(const {label,wasm,backend} of round%2?[...arms].reverse():arms) {
 const vm=new V86({wasm_path:wasm,jit_backend:backend,memory_size:32<<20,
  bios:{buffer:Uint8Array.from(fs.readFileSync("build/jit-capacity.bin")).buffer},disable_keyboard:true,disable_mouse:true,disable_speaker:true,net_device:{type:"none"},autostart:false});
 try {
  await new Promise((r,j)=>{vm.add_listener("emulator-loaded",r);vm.add_listener("emulator-error",j);});
  const cpu=vm.v86.cpu,e=cpu.wm.exports;
  const counter=()=>new Uint32Array(e.memory.buffer)[664>>2];
  const data=()=>new DataView(cpu.mem8.buffer,cpu.mem8.byteOffset);
  vm.run();let end=performance.now()+15000;
  while(data().getUint16(0x500,true)!==0xCAFE){assert(performance.now()<end);await sleep(1);} await vm.stop();
  vm.write_memory(Uint8Array.from(work.code),PC);if(work.peer)vm.write_memory(Uint8Array.from(work.peer),PEER);
  const prepare=n=>{
   cpu.segment_offsets.fill(0,0,6);cpu.segment_is_null.fill(0,0,6);cpu.is_32[0]=1;cpu.stack_size_32[0]=1;
   cpu.reg32.set([0,n,work.peer?PEER:0x31415926,work.peer?PC:7,0x90000,0,DATA,work.edi||0]);
   cpu.reg_xmm32s.fill(0);for(let lane=0;lane<4;lane++)cpu.reg_xmm32s[4+lane]=0x3F800000;
   if(work.source)cpu.reg_xmm32s.set(work.source,4);
   if(work.destination)cpu.reg_xmm32s.set(work.destination);
   cpu.flags[0]=2;cpu.flags_changed[0]=0;cpu.in_hlt[0]=0;cpu.instruction_pointer[0]=PC;
   data().setUint32(DATA,0,true);new Uint32Array(e.memory.buffer)[664>>2]=0;e.update_state_flags();
  };
  const run=async n=>{prepare(n);const start=performance.now();vm.run();const until=start+30000;
   while(!cpu.in_hlt[0]){assert(performance.now()<until,`${work.name}/${backend} timeout`);await sleep(1);}
   const timing=await finish_halted_timing(vm,start),ms=timing.ms,steps=counter();
   assert.equal(steps,n*work.per+(work.peer?0:1),"identical exact retired guest work");
   return {...timing,steps,mips:steps/ms/1000,state:{gpr:Array.from(cpu.reg32),flags:e.get_eflags(),xmm:Array.from(cpu.reg_xmm32s),data:data().getUint32(DATA,true),data16:Array.from(cpu.mem8.slice(DATA,DATA+16)),pc:cpu.instruction_pointer[0]}};
  };
  // Yield between bounded warm runs so asynchronous publications can finish.
  for(let n=0;n<20;n++){await run(20000);await sleep(1);}
  if(backend==="ir") assert.equal(e.ir_cache_entry_stat(PC,0,1,5),2,"fixed work must warm the measured entry to Tier 2");
  const ir_before=e.ir_cache_stat(10);
  const row={workload:work.name,suite,label,backend,round,wasm,scale,...await run(work.iterations)};
  row.budget_batch_blocks=e.ir_cache_entry_stat(PC,0,1,13);
  row.ir_steps=(e.ir_cache_stat(10)-ir_before)>>>0;
  row.ir_coverage=row.ir_steps/row.steps;
  if(backend==="ir") assert(row.ir_coverage>=0.95,"fixed work must actually execute through cached IR");
  const paired=results.find(r=>r.workload===work.name&&r.round===round&&r.backend!==backend);
  if(paired)assert.deepEqual(row.state,paired.state,`${work.name}: final architectural state differs`);
  results.push(row);console.log(JSON.stringify(row));
 } finally {await vm.destroy();}
}
const median=a=>a.sort((a,b)=>a-b)[Math.floor(a.length/2)];
const matrix_for=label=>workloads.map(({name})=>{const ir=median(results.filter(r=>r.label===label&&r.workload===name&&r.backend==="ir").map(r=>r.mips));const legacy=median(results.filter(r=>r.label===label&&r.workload===name&&r.backend==="legacy").map(r=>r.mips));return {name,ir,legacy,ratio:ir/legacy};});
const matrix=matrix_for("current");
const baseline=baselineWasm?matrix_for("baseline"):null;
const geomean=Math.exp(matrix.reduce((sum,r)=>sum+Math.log(r.ratio),0)/matrix.length);
console.log(JSON.stringify({event:"summary",suite,timing_scope:"start-to-observed-halt",wasm:wasmPath,baseline_wasm:baselineWasm||null,scale,repetitions,matrix,baseline,comparison:baseline?matrix.map((r,i)=>({name:r.name,current_over_baseline:r.ir/baseline[i].ir})):null,geomean,pass:geomean>=1&&matrix.every(r=>r.ratio>=0.9)}));

process.exitCode=geomean>=1&&matrix.every(r=>r.ratio>=0.9)?0:1;
