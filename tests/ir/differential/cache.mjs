import assert from "node:assert/strict";
import fs from "node:fs";
import {V86} from "../../../build/libv86.mjs";
const wasm=process.argv[2]||"build/v86-ir-cache-test.wasm";
const vm=new V86({wasm_path:wasm,memory_size:32<<20,bios:{buffer:Uint8Array.from(fs.readFileSync("build/jit-capacity.bin")).buffer},disable_keyboard:true,disable_mouse:true,disable_speaker:true,net_device:{type:"none"},autostart:false});
const sleep=ms=>new Promise(r=>setTimeout(r,ms)),u32=n=>[n&255,n>>>8&255,n>>>16&255,n>>>24];
const PC=0x100000, DATA=0x200000, OFFSET=1024;
try {
    await new Promise(r=>vm.add_listener("emulator-loaded",r));const cpu=vm.v86.cpu,e=cpu.wm.exports,table=cpu.wm.wasm_table;
    const word=a=>new DataView(cpu.mem8.buffer,cpu.mem8.byteOffset).getUint32(a,true);
    const set=(a,n)=>new DataView(cpu.mem8.buffer,cpu.mem8.byteOffset).setUint32(a,n,true);
    const count=()=>new Uint32Array(e.memory.buffer)[664>>2];
    vm.run();let deadline=performance.now()+10000;while((word(0x500)&65535)!==0xCAFE){assert(performance.now()<deadline);await sleep(1);}await vm.stop();await sleep(20);
    const clear=()=>{cpu.jit_clear_cache();e.ir_cache_collect();assert.equal(e.ir_cache_stat(1),0);};
    const prepare=(code,pc=PC)=>{
        clear();cpu.in_hlt[0]=0;cpu.flags[0]=2;cpu.flags_changed[0]=0;cpu.is_32[0]=1;cpu.stack_size_32[0]=1;
        cpu.segment_offsets.fill(0,0,6);cpu.instruction_pointer[0]=pc;cpu.reg32.set([0x7FFFFFFF,3,0,0,0x90000,0,DATA,0]);
        cpu.cr[0]=0x80010011|0;cpu.cr[3]=0x12000;cpu.cr[4]=512;
        set(0x12000,0x13003);set(0x12004,0x14003);set(0x12008,0x15003);
        for(let p=0;p<1024;p++){set(0x13000+p*4,(p<<12)|3);set(0x14000+p*4,0x400000+(p<<12)|3);}
        set(0x15000,PC|3);set(0x15004,PC+4096|3);
        e.full_clear_tlb();e.update_state_flags();vm.write_memory(Uint8Array.from(code),pc);
        new Uint32Array(e.memory.buffer)[664>>2]=0xFFFFFFFC;
    };
    const run=async(address=PC)=>{
        cpu.instruction_pointer[0]=address;cpu.in_hlt[0]=0;vm.run();const end=performance.now()+10000;
        while(!cpu.in_hlt[0]){assert(performance.now()<end,"guest halt timeout");await sleep(1);}await vm.stop();
    };
    const request=(length,tier=1,opt=1,cfg=1,budget=64,rep=8)=>cpu.ir_compile_cached(length,tier,opt,cfg,budget,rep);
    const reserve=(length=1)=>{
        const id=e.ir_compile_live(length,1,1,1,64,8);assert(id,`compile error ${e.ir_live_error()}`);
        const code=new Uint8Array(e.memory.buffer,e.ir_live_info(id,0)>>>0,e.ir_live_info(id,1)>>>0).slice();
        const slot=e.ir_cache_reserve(id);assert(slot);return {id,slot,code};
    };
    const install=async r=>{
        const {instance}=await WebAssembly.instantiate(r.code,{e:cpu.jit_imports});
        assert.equal(e.ir_cache_validate(r.id,r.slot),1);table.set(r.slot+OFFSET,instance.exports.f);assert.equal(e.ir_cache_finish(r.id,r.slot),1);
    };
    let programs=0;
    for(const recording of [0,1])for(const tier of [1,2])for(const opt of [0,1]){
        const code=[0x40,0x49,0x75,0xFC,0xA3,...u32(DATA),0xF4];prepare(code);
        e.performance_recording_enable(recording);const hits=e.ir_cache_stat(2);
        assert.equal(await request(code.length,tier,opt),true);assert.equal(e.ir_cache_stat(0),1);
        await run();assert.equal(e.ir_cache_stat(2)-hits,1);assert.equal(cpu.reg32[0]>>>0,0x80000002);assert.equal(word(DATA),0x80000002);assert.equal(count(),7);
        assert.equal(cpu.instruction_pointer[0],PC+code.length);programs++;
    }
    e.performance_recording_enable(0);
    console.log(`PASS: ${wasm}: ${programs} published CFG/store modules executed by normal CPU dispatch, both tier requests, optimization and recording modes`);
    // Entry fetch has architectural A-bit effects. A cold secondary page is not
    // eagerly fetched just because it belongs to the immutable request window.
    prepare([0xB8,...u32(0x12345678),0xF4],PC+4094);assert(await request(5));
    let fetchHits=e.ir_cache_stat(2);assert.equal(word(0x13000+(PC>>>12)*4)&32,0);
    await run(PC+4094);assert.equal(e.ir_cache_stat(2),fetchHits);assert.equal(word(0x13000+(PC>>>12)*4)&32,32);
    assert.equal(word(0x13000+(PC>>>12)*4+4)&32,32);assert.equal(cpu.reg32[0],0x12345678);
    await run(PC+4094);assert.equal(e.ir_cache_stat(2)-fetchHits,1);
    prepare([0xEB,0xFA,0x90,0x90,0x90,0x90],PC+4094);vm.write_memory(Uint8Array.of(0xF4),PC+4090);assert(await request(6));
    fetchHits=e.ir_cache_stat(2);await run(PC+4094);assert.equal(e.ir_cache_stat(2),fetchHits);
    assert.equal(word(0x13000+(PC>>>12)*4+4)&32,0,"unreachable secondary page retains clear A bit");
    e.ir_memory_read(PC+4096,1);await run(PC+4094);assert.equal(e.ir_cache_stat(2)-fetchHits,1);
    console.log(`PASS: ${wasm}: cold entry fetch sets A bits, cross-page code waits for visible translations, unreachable pages are not eagerly accessed`);
    // Code aliases its own PTE: initial translation changes ADD's opcode (03)
    // into AND (23). Revalidation must reject the pre-fetch artifact.
    prepare([0x90],PC+0x400);cpu.reg32[0]=DATA;cpu.reg32[6]=-1;set(DATA,0x12345678);
    set(0x13400,0x13003);vm.write_memory(Uint8Array.of(0xF4),0x13404);e.full_clear_tlb();
    assert(await request(2));fetchHits=e.ir_cache_stat(2);await run(PC+0x400);
    assert.equal(e.ir_cache_stat(2),fetchHits);assert.equal(cpu.reg32[6],0x12345678);assert.equal(e.ir_cache_stat(0),0);
    prepare([0x8B,0x06,0xF4]);set(0x13000+(DATA>>>12)*4,0);
    cpu.idtr_offset[0]=0x2000;cpu.idtr_size[0]=0x7FF;set(0x2000+14*8,8<<16);set(0x2004+14*8,0x180000|0x8E00);
    vm.write_memory(Uint8Array.of(0xF4),0x180000);assert(await request(2));fetchHits=e.ir_cache_stat(2);await run();
    assert.equal(e.ir_cache_stat(2)-fetchHits,1);assert.equal(cpu.cr[2]>>>0,DATA);assert.equal(cpu.instruction_pointer[0],0x180001);
    assert.deepEqual([0,4,8,12].map(n=>word(0x8FFF0+n)),[0,PC,8,2]);assert.equal(count(),0xFFFFFFFD);assert.equal(e.ir_cache_stat(0),0);
    console.log(`PASS: ${wasm}: fetch A-bit writes invalidate code/PTE aliases and an admitted data #PF preserves the exact exception frame without retiring the faulting instruction`);
    // Pending slots cannot execute; identity and phase validation precede installation.
    prepare([0x40,0xF4]);let r=reserve();const empty=table.get(r.slot+OFFSET);assert.equal(empty,null);
    assert.equal(e.ir_cache_finish(r.id,r.slot),0);assert.equal(e.ir_cache_validate(r.id,r.slot+65536),0);
    assert.equal(e.ir_cache_validate(r.id+1n,r.slot),0);let hits=e.ir_cache_stat(2);await run();assert.equal(e.ir_cache_stat(2),hits);
    // Publication is valid after the CPU moves; admission still needs the saved entry.
    await install(r);cpu.reg32[0]=5;await run();assert.equal(cpu.reg32[0],6);assert.equal(e.ir_cache_stat(2)-hits,1);
    assert.equal(e.ir_cache_validate(r.id,r.slot),0);assert.equal(e.ir_cache_finish(r.id,r.slot),0);
    // Same-byte host writes invalidate, and a pending cancellation can reuse the slot.
    prepare([0x40,0xF4]);r=reserve();vm.write_memory(Uint8Array.of(0x40),PC);assert.equal(e.ir_cache_validate(r.id,r.slot),0);e.ir_cache_collect();
    let newer=reserve();assert.equal(newer.slot,r.slot);assert.equal(e.ir_cache_validate(r.id,r.slot),0);assert.equal(e.ir_cache_cancel(r.id,r.slot),0);await install(newer);
    hits=e.ir_cache_stat(2);await run();assert.equal(e.ir_cache_stat(2)-hits,1);
    // Read-only byte/mapping re-observation catches unnotified writes and stale TLB semantics.
    prepare([0x40,0xF4]);assert(await request(1));cpu.mem8[PC]=0x48;hits=e.ir_cache_stat(2);await run();assert.equal(cpu.reg32[0],0x7FFFFFFE);assert.equal(e.ir_cache_stat(2),hits);
    prepare([0x40,0xF4]);assert(await request(1));vm.write_memory(Uint8Array.of(0x40,0xF4),PC+4096);
    set(0x13000+(PC>>>12)*4,PC+4096|3);e.full_clear_tlb();hits=e.ir_cache_stat(2);await run();assert.equal(e.ir_cache_stat(2),hits);
    // Cross-page dependency watch must reject even a same-byte guest write on page two.
    prepare([0xB8,...u32(0x12345678),0xF4],PC+4094);assert(await request(5));
    vm.write_memory(Uint8Array.from([0xC6,0x05,...u32(PC+4096),0x56,0xF4]),PC+0x2000);hits=e.ir_cache_stat(2);await run(PC+0x2000);
    assert.equal(e.ir_cache_stat(0),0);await run(PC+4094);assert.equal(e.ir_cache_stat(2),hits);assert.equal(cpu.reg32[0],0x12345678);
    // A store into the currently executing region exits before stale trailing bytes.
    prepare([0xC6,0x05,...u32(PC+7),0x48,0x40,0xF4]);assert(await request(9));hits=e.ir_cache_stat(2);await run();
    assert.equal(e.ir_cache_stat(2)-hits,1);assert.equal(cpu.reg32[0],0x7FFFFFFE);assert.equal(e.ir_cache_stat(1),0);
    // A failed replacement must retain the existing published entry.
    prepare([0x40,0xF4]);assert(await request(1));r=reserve();assert.equal(e.ir_cache_cancel(r.id,r.slot),1);e.ir_cache_collect();
    hits=e.ir_cache_stat(2);await run();assert.equal(e.ir_cache_stat(2)-hits,1);
    // Reset/restore drop entries; an old pending browser completion cannot reinstall them.
    prepare([0x40,0xF4]);r=reserve();const snapshot=await vm.save_state();await vm.restore_state(snapshot);
    assert.equal(e.ir_cache_validate(r.id,r.slot),0);e.ir_cache_collect();assert.equal(e.ir_cache_stat(1),0);
    console.log(`PASS: ${wasm}: pending/duplicate/forged/ABA publication, moved IP, raw and same-byte guest/host writes, physical remapping, secondary dependencies, active SMC and restore`);
    // Keep the actual running function/reservation alive through a synchronous I/O callback.
    for(const reset of [false,true]){
        prepare([0xEE,0xF4]);cpu.reg32[2]=0x502;r=reserve();await install(r);let callbacks=0;
        const f=table.get(r.slot+OFFSET);
        cpu.io.register_write(0x502,null,()=>{
            callbacks++;if(reset)cpu.jit_clear_cache();else vm.write_memory(Uint8Array.of(0xEE),PC);
            assert.equal(e.ir_cache_stat(0),0);assert.equal(e.ir_cache_stat(1),1);
            assert.equal(e.ir_cache_collect(),0);assert.equal(table.get(r.slot+OFFSET),f);
            assert.equal(e.ir_compile_live(1,1,1,1,64,8),0n);assert.equal(e.ir_live_error(),2);
            assert.equal(e.ir_cache_reserve(r.id),0);
        });
        await run();assert.equal(callbacks,1);assert.equal(e.ir_cache_stat(1),0);assert.equal(table.get(r.slot+OFFSET),null);
    }
    prepare([0xF3,0xA4,0xF4]);cpu.reg32[7]=DATA+16;vm.write_memory(Uint8Array.of(11,22,33),DATA);
    assert(await request(2,1,1,1,64,0));hits=e.ir_cache_stat(2);await run();
    assert.equal(e.ir_cache_stat(2)-hits,1);assert.equal(cpu.reg32[1],0);assert.deepEqual(Array.from(vm.read_memory(DATA+16,3)),[11,22,33]);
    assert.equal(e.ir_cache_stat(1),0);
    console.log(`PASS: ${wasm}: active I/O write/reset retains its table slot until return, rejects nested compilation, and zero-retirement REP exits resume interpretation`);
    // Table capacity accounting includes IR reservations while legacy compilation continues.
    prepare([0x40,0xF4]);const free=e.jit_get_wasm_table_index_free_list_count();
    for(let i=0;i<32;i++){const address=PC+i*4096;vm.write_memory(Uint8Array.of(0x40,0xF4),address);cpu.instruction_pointer[0]=address;assert(await request(1));}
    assert.equal(e.ir_cache_stat(0),32);assert.equal(e.jit_get_wasm_table_index_free_list_count(),free-32);
    cpu.instruction_pointer[0]=PC+32*4096;vm.write_memory(Uint8Array.of(0x40,0xF4),PC+32*4096);assert.equal(await request(1),false);
    if(e.jit_force_generate_unsafe){
        for(let i=0;i<900;i++){
            const address=PC+(40+i)*4096;vm.write_memory(Uint8Array.from([0x40,0xF4]),address);cpu.instruction_pointer[0]=address;
            await new Promise((resolve,reject)=>{
                const timer=setTimeout(()=>reject(new Error("legacy publication timeout")),10000);
                cpu.test_hook_did_finalize_wasm=()=>{clearTimeout(timer);resolve();};
                try{assert(e.jit_force_generate_unsafe(address));}catch(error){clearTimeout(timer);reject(error);}
            });
        }
        cpu.test_hook_did_finalize_wasm=undefined;assert.equal(e.ir_cache_stat(0),32);
        hits=e.ir_cache_stat(2);await run(PC);assert.equal(e.ir_cache_stat(2)-hits,1);
        console.log(`PASS: ${wasm}: 900 actual legacy publications/evictions preserve all 32 IR reservations and an executable IR entry`);
    }
    clear();assert.equal(e.jit_get_wasm_table_index_free_list_count(),899);
    console.log(`PASS: ${wasm}: 32 IR entries share the bounded 899-slot pool, capacity rejection and complete reclamation, legacy coexistence`);
    // Browser failures and out-of-order completion use the production JS bridge.
    const original=WebAssembly.instantiate;
    try {
        if(e.jit_force_generate_unsafe){
            prepare([0x40,0xF4]);r=reserve();let checked=0;
            WebAssembly.instantiate=(code,imports)=>{
                // The legacy generator holds JIT_STATE during this host call.
                assert.equal(e.ir_cache_cancel(r.id,r.slot),1);
                assert.equal(e.ir_cache_collect(),0);assert.equal(e.ir_cache_stat(1),1);
                assert.equal(e.ir_cache_validate(r.id,r.slot),0);checked++;
                return original(code,imports);
            };
            await new Promise((resolve,reject)=>{
                const timer=setTimeout(()=>reject(new Error("legacy bridge callback timeout")),10000);
                cpu.test_hook_did_finalize_wasm=()=>{clearTimeout(timer);resolve();};
                try{assert(e.jit_force_generate_unsafe(PC));}catch(error){clearTimeout(timer);reject(error);}
            });
            cpu.test_hook_did_finalize_wasm=undefined;assert.equal(checked,1);e.ir_cache_collect();assert.equal(e.ir_cache_stat(1),0);
            console.log(`PASS: ${wasm}: publication/collection refuse a synchronous legacy-generator host callback while its JIT lock is held`);
        }
        for(const kind of ["sync","async","missing","table"]){
            prepare([0x40,0xF4]);
            WebAssembly.instantiate=kind==="sync"?()=>{throw new WebAssembly.CompileError("controlled");}:kind==="async"?()=>Promise.reject(new WebAssembly.CompileError("controlled")):kind==="missing"?()=>Promise.resolve({instance:{exports:{}}}):original;
            const oldSet=table.set;
            if(kind==="table")table.set=(index,f)=>{if(f!==null)throw new TypeError("controlled table failure");return oldSet.call(table,index,f);};
            try{assert.equal(await request(1),false);}finally{table.set=oldSet;}
            e.ir_cache_collect();assert.equal(e.ir_cache_stat(1),0);assert.equal(e.jit_get_wasm_table_index_free_list_count(),899);
        }
        prepare([0x40,0xF4]);const queued=[];WebAssembly.instantiate=(code,imports)=>new Promise((resolve,reject)=>queued.push({code,imports,resolve,reject}));
        const old=request(1);assert.equal(queued.length,1);clear();const replacement=request(1);assert.equal(queued.length,2);
        queued[1].resolve(await original(queued[1].code,queued[1].imports));assert.equal(await replacement,true);
        queued[0].resolve(await original(queued[0].code,queued[0].imports));assert.equal(await old,false);
        hits=e.ir_cache_stat(2);await run();assert.equal(e.ir_cache_stat(2)-hits,1);
        for(const changed of ["wasm","exports","table"]){
            prepare([0x40,0xF4]);const pending=request(1),job=queued.at(-1);
            const result=await original(job.code,job.imports),owner=cpu.wm,ownerExports=owner.exports,ownerTable=owner.wasm_table;
            try{
                if(changed==="wasm")cpu.wm={...owner};else if(changed==="exports")owner.exports={...ownerExports};else owner.wasm_table={};
                job.resolve(result);assert.equal(await pending,false);assert.equal(e.ir_cache_stat(0),0);
            }finally{cpu.wm=owner;owner.exports=ownerExports;owner.wasm_table=ownerTable;}
            clear();assert.equal(e.jit_get_wasm_table_index_free_list_count(),899);
        }
    }finally{WebAssembly.instantiate=original;}
    console.log(`PASS: ${wasm}: synchronous/asynchronous browser failures, missing exports, table failure, changed VM/export/table identities and late completion through the actual publication bridge`);
} finally { await vm.destroy(); }
