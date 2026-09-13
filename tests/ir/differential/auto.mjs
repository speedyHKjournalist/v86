import assert from "node:assert/strict";
import fs from "node:fs";
import {V86} from "../../../build/libv86.mjs";
const wasm=process.argv[2]||"build/v86-ir-cache-test.wasm",PC=0x100000;
const vm=new V86({wasm_path:wasm,memory_size:32<<20,bios:{buffer:Uint8Array.from(fs.readFileSync("build/jit-capacity.bin")).buffer},disable_keyboard:true,disable_mouse:true,disable_speaker:true,net_device:{type:"none"},autostart:false});
const sleep=ms=>new Promise(r=>setTimeout(r,ms)),u32=n=>[n&255,n>>>8&255,n>>>16&255,n>>>24];
const until=async(test,label)=>{const end=performance.now()+15000;while(!test()){assert(performance.now()<end,label);await sleep(1);}};
try{
    await new Promise(r=>vm.add_listener("emulator-loaded",r));const cpu=vm.v86.cpu,e=cpu.wm.exports;
    const word=a=>new DataView(cpu.mem8.buffer,cpu.mem8.byteOffset).getUint32(a,true);
    vm.run();await until(()=>(word(0x500)&65535)===0xCAFE,"boot");await vm.stop();await sleep(20);
    const configure=(enabled=1,threshold=2,promote=4)=>assert.equal(e.ir_auto_config(enabled,threshold,promote,192,256,64),1);
    const stats=()=>Array.from({length:12},(_,i)=>e.ir_auto_stat(i));
    const publisher=cpu.ir_auto_publish;let coldPublications=0;
    cpu.ir_auto_publish=function(id,slot,ptr,len){
        assert.equal(e.ir_entry_matches(cpu.instruction_pointer[0],cpu.segment_offsets[1],cpu.is_32[0]),1,"publication starts outside legacy guest frames");
        assert.equal(e.ir_auto_stat(10),1);e.ir_auto_complete(id,1);assert.equal(e.ir_auto_stat(10),1,"premature completion cannot consume a pending task");
        coldPublications++;return publisher.call(this,id,slot,ptr,len);
    };
    const prepare=async(code=[0x40,0xEB,0xFD],address=PC)=>{
        await vm.stop();configure(0);cpu.jit_clear_cache();e.ir_cache_collect();
        cpu.in_hlt[0]=0;cpu.flags[0]=2;cpu.flags_changed[0]=0;cpu.segment_offsets.fill(0,0,6);cpu.is_32[0]=1;cpu.stack_size_32[0]=1;
        cpu.reg32.fill(0);cpu.reg32[4]=0x90000;cpu.instruction_pointer[0]=address;e.update_state_flags();
        vm.write_memory(Uint8Array.from(code),address);new Uint32Array(e.memory.buffer)[664>>2]=0;
    };
    assert.equal(e.ir_auto_stat(11),0);assert.equal(e.ir_auto_config(2,1,1,192,256,64),0);assert.equal(e.ir_auto_config(1,0,1,192,256,64),0);
    for(const recording of [0,1]){
        await prepare();e.performance_recording_enable(recording);const before=stats(),hits=e.ir_cache_stat(2);configure();vm.run();
        await until(()=>e.ir_auto_stat(5)>before[5]&&e.ir_cache_stat(2)>hits,"automatic Tier 1/2 publication");await vm.stop();
        const count=new Uint32Array(e.memory.buffer)[664>>2],eax=cpu.reg32[0]>>>0,atJump=cpu.instruction_pointer[0]===PC+1;
        assert.equal(count,(eax*2-(atJump?1:0))>>>0);assert([PC,PC+1].includes(cpu.instruction_pointer[0]));
        assert(e.ir_auto_stat(2)>before[2]&&e.ir_auto_stat(3)>before[3]&&e.ir_auto_stat(4)>before[4]);
    }
    e.performance_recording_enable(0);
    console.log(`PASS: ${wasm}: automatic Tier 1 compilation, safe optimized promotion and actual CPU execution with exact loop counts, recording off/on`);
    await prepare();const disabled=e.get_jit_config(0),legacyPublisher=cpu.codegen_finalize;let legacyCalls=0;
    cpu.codegen_finalize=function(...args){legacyCalls++;return legacyPublisher.apply(this,args);};e.set_jit_config(0,1);
    try{
        const independent=stats(),independentHits=e.ir_cache_stat(2);configure();vm.run();await until(()=>e.ir_auto_stat(5)>independent[5]&&e.ir_cache_stat(2)>independentHits,"IR with legacy generation disabled");await vm.stop();
        assert.equal(legacyCalls,0);const n=new Uint32Array(e.memory.buffer)[664>>2];assert.equal(n,(cpu.reg32[0]*2-(cpu.instruction_pointer[0]===PC+1?1:0))>>>0);
    }finally{await vm.stop();e.set_jit_config(0,disabled);cpu.codegen_finalize=legacyPublisher;}
    console.log(`PASS: ${wasm}: automatic IR compilation/promotion executes with legacy generation disabled and zero calls to the legacy publisher`);
    await prepare();cpu.is_32[0]=0;cpu.segment_offsets[1]=PC-0x1000;cpu.reg32[0]=0x76540000;e.update_state_flags();let modeBefore=stats();configure();vm.run();
    await until(()=>e.ir_auto_stat(5)>modeBefore[5],"16-bit automatic promotion");await vm.stop();
    const modeCount=new Uint32Array(e.memory.buffer)[664>>2];assert([PC,PC+1].includes(cpu.instruction_pointer[0]));
    assert.equal(cpu.reg32[0]>>>0,(0x76540000|((modeCount+(cpu.instruction_pointer[0]===PC+1?1:0))/2&65535))>>>0);
    console.log(`PASS: ${wasm}: 16-bit automatic compilation/promotion retains CS-relative entry and AX partial-register semantics`);
    await prepare([0x40,...Array(40).fill(0x90),0xE9,...u32(-46)]);let longBefore=stats();configure();vm.run();
    await until(()=>e.ir_auto_stat(5)>longBefore[5],"larger Tier 2 region");await vm.stop();
    const offset=cpu.instruction_pointer[0]-PC;assert(offset>=0&&offset<=41);
    assert.equal(new Uint32Array(e.memory.buffer)[664>>2],((cpu.reg32[0]-(offset?1:0))*42+offset)>>>0);
    console.log(`PASS: ${wasm}: a 42-instruction loop spans lightweight regions and preserves exact retirement through the larger optimized region`);
    // Unsupported x87 semantics are suppressed until their actual source changes.
    await prepare([0xD9,0xEE,0xDD,0xD8,0xEB,0xFA]);let before=stats();configure();vm.run();
    await until(()=>e.ir_auto_stat(6)>before[6]&&e.ir_auto_stat(8)>before[8],"compile-stop suppression");await sleep(80);
    const attempts=e.ir_auto_stat(2)+e.ir_auto_stat(3);await sleep(100);assert.equal(e.ir_auto_stat(2)+e.ir_auto_stat(3),attempts);
    vm.write_memory(Uint8Array.of(0x40,0xEB,0xFD),PC);await until(()=>e.ir_auto_stat(4)>before[4],"recompile changed code");await vm.stop();
    console.log(`PASS: ${wasm}: unchanged unsupported lowering is not repeatedly compiled; host code writes enable recompilation`);
    // Browser rejection of an upgrade retains the working Tier 1 entry.
    const original=WebAssembly.instantiate;
    const isIR=code=>WebAssembly.Module.imports(new WebAssembly.Module(code)).some(i=>i.name==="ir_entry_matches");
    try{
        await prepare();before=stats();configure(1,2,1000000);vm.run();await until(()=>e.ir_auto_stat(4)>before[4],"Tier 1 before rejected upgrade");await vm.stop();
        WebAssembly.instantiate=(code,imports)=>isIR(code)?Promise.reject(new WebAssembly.CompileError("controlled auto failure")):original(code,imports);
        before=stats();let hits=e.ir_cache_stat(2);configure(1,2,2);vm.run();await until(()=>e.ir_auto_stat(7)>before[7]&&e.ir_auto_stat(8)>before[8],"failed promotion suppression");
        const failedAttempts=e.ir_auto_stat(3);await sleep(100);assert.equal(e.ir_auto_stat(3),failedAttempts);assert(e.ir_cache_stat(2)>hits);await vm.stop();
    }finally{WebAssembly.instantiate=original;}
    console.log(`PASS: ${wasm}: browser upgrade failure is suppressed while the existing Tier 1 entry keeps executing`);
    // One in-flight automatic task; restore/reset can supersede its owner.
    try{
        await prepare();before=stats();const queued=[];
        WebAssembly.instantiate=(code,imports)=>isIR(code)?new Promise((resolve,reject)=>queued.push({code,imports,resolve,reject})):original(code,imports);
        configure();vm.run();await until(()=>queued.length===1,"pending auto task");await sleep(70);assert.equal(queued.length,1);assert.equal(e.ir_auto_stat(10),1);
        cpu.jit_clear_cache();await until(()=>queued.length===2,"new generation auto task");
        queued[1].resolve(await original(queued[1].code,queued[1].imports));await until(()=>e.ir_auto_stat(4)>before[4],"replacement publication");
        configure(0);queued[0].resolve(await original(queued[0].code,queued[0].imports));await sleep(30);assert.equal(e.ir_auto_stat(10),0);assert.equal(e.ir_cache_stat(0),1);await vm.stop();
    }finally{WebAssembly.instantiate=original;}
    console.log(`PASS: ${wasm}: one pending automatic task, reset supersession and stale out-of-order completion`);
    await prepare();before=stats();configure();vm.run();await until(()=>e.ir_auto_stat(4)>before[4],"before save");await vm.stop();
    const saved=await vm.save_state();await vm.restore_state(saved);assert.equal(e.ir_auto_stat(11),1);assert.equal(e.ir_cache_stat(0),0);before=stats();vm.run();
    await until(()=>e.ir_auto_stat(4)>before[4],"automatic rebuild after snapshot restore");await vm.stop();
    console.log(`PASS: ${wasm}: snapshot restore drops runtime entries and automatic compilation rebuilds them`);
    if(e.jit_force_generate_unsafe){
        await prepare([0x40,0xFF,0xE2]);const other=PC+0x2000;vm.write_memory(Uint8Array.of(0x41,0xFF,0xE3),other);cpu.reg32[2]=other;cpu.reg32[3]=PC;
        const pages=e.get_jit_config(1),tiered=e.get_jit_config(7);e.set_jit_config(1,1);e.set_jit_config(7,0);e.set_jit_config(5,1);
        for(const address of [PC,other]){
            cpu.instruction_pointer[0]=address;
            await new Promise((resolve,reject)=>{
                const timer=setTimeout(()=>reject(new Error("linked legacy warmup timeout")),10000);
                cpu.test_hook_did_finalize_wasm=()=>{clearTimeout(timer);resolve();};
                try{assert(e.jit_force_generate_unsafe(address));}catch(error){clearTimeout(timer);reject(error);}
            });
        }
        cpu.test_hook_did_finalize_wasm=undefined;cpu.instruction_pointer[0]=PC;before=stats();const links=e.get_jit_link_count();configure(1,32,1000000);vm.run();
        await until(()=>e.ir_auto_stat(4)>=before[4]+2,"linked heat reaches cold compilation");await vm.stop();
        assert(e.ir_auto_stat(1)>before[1]);assert(e.get_jit_link_count()>links);e.set_jit_config(1,pages);e.set_jit_config(7,tiered);
        console.log(`PASS: ${wasm}: recording-off legacy links accumulate entry heat; both linked targets compile only after returning to cold dispatch`);
    }
    // Automatic eviction preserves manually published entries and the shared pool bound.
    await prepare([0x40,0xF4]);assert(await cpu.ir_compile_cached(1,1,1,1,64,8));configure(1,1,1000000);
    for(let i=1;i<=40;i++){
        await vm.stop();const address=PC+i*4096;vm.write_memory(Uint8Array.of(0x40,0xEB,0xFD),address);cpu.instruction_pointer[0]=address;cpu.in_hlt[0]=0;
        before=stats();const entryHits=e.ir_cache_stat(2);vm.run();await until(()=>e.ir_auto_stat(4)>before[4]&&e.ir_cache_stat(2)>entryHits,`automatic capacity entry ${i}`);assert(e.ir_cache_stat(1)<=32);
    }
    await sleep(40);await until(()=>e.ir_auto_stat(10)===0,"capacity publications settle");
    const stationary=e.ir_auto_stat(2);await sleep(80);assert.equal(e.ir_auto_stat(2),stationary,"evicted inactive entries do not recompile from historical heat");
    await vm.stop();configure(0);assert.equal(e.ir_cache_stat(0),32);cpu.instruction_pointer[0]=PC;cpu.in_hlt[0]=0;let hits=e.ir_cache_stat(2);vm.run();await until(()=>cpu.in_hlt[0],"retained explicit entry");await vm.stop();assert.equal(e.ir_cache_stat(2)-hits,1);
    assert(e.ir_auto_stat(9)<=128);cpu.jit_clear_cache();e.ir_cache_collect();assert.equal(e.jit_get_wasm_table_index_free_list_count(),899);
    console.log(`PASS: ${wasm}: 40 automatically compiled entries use bounded eviction while preserving an explicit entry; premature completion and cold publication guards (${coldPublications} publications)`);
}finally{await vm.destroy();}
