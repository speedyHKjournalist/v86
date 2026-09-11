// Fresh-VM cold/steady comparison plus promotion, SMC and restore checks.
import assert from "node:assert/strict";
import fs from "node:fs";
import { V86 } from "../../build/libv86.mjs";
const baseline=process.argv[2] || "build/v86.wasm";
const candidate=process.argv[3] || "build/v86.wasm";
const bios=Uint8Array.from(fs.readFileSync("build/jit-capacity.bin")).buffer;
const u32=n=>[n&255,n>>>8&255,n>>>16&255,n>>>24&255];
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
const results=[[],[]];
const word=(vm,a)=>new DataView(Uint8Array.from(vm.read_memory(a,4)).buffer).getUint32(0,true);
for(let round=0;round<5;round++) for(const index of round&1 ? [1,0] : [0,1]) {
    const vm=new V86({wasm_path:index ? candidate : baseline,bios:{buffer:bios.slice(0)},memory_size:32<<20,
        disable_keyboard:true,disable_mouse:true,disable_speaker:true,net_device:{type:"none"},autostart:false});
    try {
        await new Promise(r=>vm.add_listener("emulator-loaded",r));
        vm.run();
        const boot=performance.now();
        while(word(vm,0x500)!==0xCAFE) { assert(performance.now()-boot<10000);await sleep(1); }
        await vm.stop();
        const cpu=vm.v86.cpu,e=cpu.wm.exports;
        e.set_jit_config(7,index);
        for(let page=0;page<6;page++) {
            const address=0x100000+page*4096,p=[0xB8,...u32(page+1),0xBB,...u32(17)];
            for(let i=0;i<32;i++) p.push(0x01,0xD8,0x29,0xD8);
            p.push(0xA3,...u32(0x210000+page*4),0xFF,0x05,...u32(0x600));
            p.push(0xE9,...u32(0x100000+(page+1)%6*4096-address-p.length-5));
            vm.write_memory(Uint8Array.from(p),address);
        }
        cpu.instruction_pointer[0]=0x100000;cpu.in_hlt[0]=0;
        e.performance_recording_enable(1);
        const start=performance.now();vm.run();
        while(e.performance_recording_get(1)===0) { assert(performance.now()-start<10000);await sleep(1); }
        const first_jit_ms=performance.now()-start;
        await sleep(150);await vm.stop();
        const interpreted_steps=e.performance_recording_get(0),codegen_ms=e.performance_recording_get(5);
        e.performance_recording_enable(0);
        for(let page=0;page<6;page++) assert.equal(word(vm,0x210000+page*4),page+1);
        const before=word(vm,0x600),warm=performance.now();
        while(performance.now()-warm<120) cpu.main_loop();
        const pages_per_ms=((word(vm,0x600)-before)>>>0)/(performance.now()-warm);
        const tier1=e.get_jit_tier1_compiles?.() || 0,tier2=e.get_jit_tier2_compiles?.() || 0;
        results[index].push({first_jit_ms,interpreted_steps,codegen_ms,pages_per_ms,tier1,tier2});
        if(index) { assert(tier1>0,"tier 1 compiled");assert(tier2>0,"hot modules promoted"); }
        if(index && round===0) {
            const saved=await vm.save_state();
            vm.write_memory(Uint8Array.from(u32(9)),0x102001);
            cpu.instruction_pointer[0]=0x100000;cpu.in_hlt[0]=0;vm.run();await sleep(60);await vm.stop();
            assert.equal(word(vm,0x210008),9,"SMC after promotion uses new bytes");
            await vm.restore_state(saved);vm.run();await sleep(60);await vm.stop();
            assert.equal(word(vm,0x210008),3,"restore discards promoted code");
        }
    } finally { await vm.destroy(); }
}
const median=a=>a.toSorted((a,b)=>a-b)[a.length>>1];
for(let i=0;i<2;i++) {
    const summary={wasm:i ? candidate : baseline,tiered:!!i,rounds:5};
    for(const key of Object.keys(results[i][0])) summary[key]=median(results[i].map(x=>x[key]));
    console.log(JSON.stringify({...summary,samples:results[i]}));
}
console.log("PASS: tier promotion, code modification and save/restore");
