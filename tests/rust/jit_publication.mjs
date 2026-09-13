import assert from "node:assert/strict";
import fs from "node:fs";
import {V86} from "../../build/libv86.mjs";
const wasm=process.argv[2]||"build/v86-jit-test.wasm";
const vm=new V86({wasm_path:wasm,memory_size:32<<20,bios:{buffer:Uint8Array.from(fs.readFileSync("build/jit-capacity.bin")).buffer},disable_keyboard:true,disable_mouse:true,disable_speaker:true,net_device:{type:"none"},autostart:false});
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
const nativeInstantiate=WebAssembly.instantiate;
let table,oldSet;
try {
    await new Promise(r=>vm.add_listener("emulator-loaded",r));
    const cpu=vm.v86.cpu,e=cpu.wm.exports,mem=cpu.mem8;
    const view=new DataView(mem.buffer,mem.byteOffset),word=a=>view.getUint32(a,true);
    vm.run(); const deadline=performance.now()+10000;
    while(view.getUint16(0x500,true)!==0xCAFE){assert(performance.now()<deadline);await sleep(1);}await vm.stop();await sleep(20);cpu.jit_clear_cache();
    e.performance_recording_enable(1);
    table=cpu.wm.wasm_table;oldSet=table.set;
    const requests=[],writes=[];let failInstall=false,corruptNext=false,throwNext=false;
    table.set=function(slot,fn){writes.push([slot,fn]);if(fn && failInstall){failInstall=false;throw new Error("injected table installation failure");}return oldSet.call(this,slot,fn);};
    WebAssembly.instantiate=(bytes,imports)=>{
        let resolve,reject;
        const promise=new Promise((a,b)=>{resolve=a;reject=b;});
        const native=nativeInstantiate(corruptNext?Uint8Array.of(0):bytes,imports);corruptNext=false;native.catch(()=>{});
        requests.push({bytes:Uint8Array.from(bytes),buffer:bytes,native,resolve,reject});
        if(throwNext){throwNext=false;throw new TypeError("injected synchronous browser failure");}
        return promise;
    };
    const finalize=cpu.codegen_finalize;
    cpu.codegen_finalize=function(...args){const result=finalize.apply(this,args);const request=requests.at(-1);request.args=args;request.done=Promise.resolve(result);return result;};
    const BASE=0x100000,OTHER=BASE+4096,OFFSET=1024;
    const program=value=>{const bytes=Uint8Array.from([0xB8,0,0,0,0,0xA3,0,6,0,0,0xF4]);new DataView(bytes.buffer).setUint32(1,value,true);return bytes;};
    const put=(address,value)=>vm.write_memory(program(value),address);
    const force=address=>{const n=requests.length;assert(e.jit_force_generate_unsafe(address));assert.equal(requests.length,n+1);return requests.at(-1);};
    const key=r=>[r.args[0],r.args[1],r.args[2],r.args[5],r.args[6]];
    const fn=r=>table.get(OFFSET+r.args[0]);
    const complete=async r=>{r.resolve(await r.native);return await r.done;};
    const reject=async r=>{r.reject(new Error("injected browser compilation failure"));assert.equal(await r.done,false);};
    const assertNoInstall=(begin)=>assert.equal(writes.slice(begin).filter(([,fn])=>fn).length,0);
    const execute=async(address,value,compiled=false)=>{const count=e.performance_recording_get(1);view.setUint32(0x600,0,true);cpu.instruction_pointer[0]=address;cpu.in_hlt[0]=0;vm.run();const until=performance.now()+10000;while(!cpu.in_hlt[0]){assert(performance.now()<until,"publication guest execution timeout");await sleep(1);}await vm.stop();assert.equal(word(0x600),value);if(compiled)assert(e.performance_recording_get(1)>count,"retained module still executes through JIT");};

    put(BASE,1);let r=force(BASE),begin=writes.length;
    put(BASE,2);assert.equal(await complete(r),false);assert.equal(fn(r),null);assertNoInstall(begin);
    r=force(BASE);assert.equal(await complete(r),true);await execute(BASE,2);
    cpu.jit_clear_cache();put(BASE,3);r=force(BASE);begin=writes.length;cpu.jit_clear_cache();
    assert.equal(await complete(r),false);assertNoInstall(begin);

    put(BASE,30);const cancelled=force(BASE);cpu.jit_clear_cache();put(BASE,31);const replacement=force(BASE);
    assert.equal(cancelled.args[0],replacement.args[0]);assert.deepEqual(key(cancelled).slice(0,3),key(replacement).slice(0,3));
    assert.equal(await complete(replacement),true);const replacementFn=fn(replacement);begin=writes.length;
    assert.equal(await complete(cancelled),false);assert.equal(fn(replacement),replacementFn);assert.equal(writes.length,begin);
    cpu.jit_clear_cache();

    put(BASE,4);r=force(BASE);const before=e.jit_publication_stat(0);await reject(r);
    assert.equal(e.jit_publication_stat(0),before+1);assert.equal(fn(r),null);
    const n=requests.length;for(let i=0;i<20;i++)assert.equal(e.jit_force_generate_unsafe(BASE),0);
    assert.equal(requests.length,n,"unchanged failed code must not retry indefinitely");
    put(BASE,5);r=force(BASE);assert.equal(await complete(r),true);await execute(BASE,5);

    cpu.jit_clear_cache();put(BASE,6);const old=force(BASE);
    assert(e.codegen_finalize_failed(...key(old)));put(OTHER,7);const newer=force(OTHER);
    assert.equal(newer.args[0],old.args[0],"test forces slot reuse");
    assert.deepEqual(old.buffer,old.bytes,"pending Wasm bytes survive builder reuse");
    assert.notDeepEqual(key(newer).slice(3),key(old).slice(3));
    assert.equal(await complete(newer),true);const installed=fn(newer);begin=writes.length;
    assert.equal(await complete(old),false);assert.equal(fn(newer),installed);assert.equal(writes.length,begin,"late callback cannot even clear reused slot");
    assert.equal(e.codegen_finalize_failed(...key(old)),0);e.codegen_finalize_finished(...key(old));assert.equal(fn(newer),installed);
    await execute(OTHER,7);

    cpu.jit_clear_cache();put(BASE,8);r=force(BASE);
    for(let field=0;field<5;field++){const bad=key(r);bad[field]^=field===1?4096:1;assert.equal(e.codegen_finalize_validate(...bad),0);assert.equal(e.codegen_finalize_failed(...bad),0);e.codegen_finalize_finished(...bad);}
    for(const [field,extra] of [[0,65536],[2,256]]){const bad=key(r);bad[field]+=extra;assert.equal(e.codegen_finalize_validate(...bad),0);assert.equal(e.codegen_finalize_failed(...bad),0);e.codegen_finalize_finished(...bad);}
    e.codegen_finalize_finished(...key(r));assert.equal(fn(r),null,"finish without validation cannot publish");
    assert.equal(await complete(r),true);const stable=fn(r);e.codegen_finalize_finished(...key(r));assert.equal(fn(r),stable);
    cpu.jit_clear_cache();put(BASE,9);r=force(BASE);failInstall=true;assert.equal(await complete(r),false);assert.equal(fn(r),null);
    assert.equal(e.jit_force_generate_unsafe(BASE),0);cpu.jit_clear_cache();r=force(BASE);assert.equal(await complete(r),true);

    cpu.jit_clear_cache();put(BASE,42);r=force(BASE);put(BASE,43);await reject(r);
    r=force(BASE);assert.equal(await complete(r),true,"late failure for written code cannot suppress changed code");

    cpu.jit_clear_cache();put(BASE,40);r=force(BASE);r.resolve({instance:{exports:{}}});
    assert.equal(await r.done,false);assert.equal(fn(r),null);
    cpu.jit_clear_cache();put(BASE,41);corruptNext=true;r=force(BASE);
    const invalidModule=await r.native.catch(error=>error);assert(invalidModule instanceof WebAssembly.CompileError);
    r.reject(invalidModule);assert.equal(await r.done,false);assert.equal(fn(r),null);

    cpu.jit_clear_cache();put(BASE,44);throwNext=true;const syncFailures=e.jit_publication_stat(0);r=force(BASE);
    assert.equal(e.jit_publication_stat(0),syncFailures,"synchronous browser error must not reenter the locked generator");
    assert.equal(await r.done,false);assert.equal(e.jit_publication_stat(0),syncFailures+1);assert.equal(fn(r),null);
    assert.equal(e.jit_force_generate_unsafe(BASE),0);put(BASE,45);r=force(BASE);
    assert.equal(await complete(r),true);await execute(BASE,45);

    cpu.jit_clear_cache();put(BASE,46);put(BASE+64,47);const retained=force(BASE);
    assert.equal(await complete(retained),true);const retainedFn=fn(retained);r=force(BASE+64);
    assert.notEqual(r.args[0],retained.args[0]);await reject(r);
    assert.equal(fn(r),null);assert.equal(fn(retained),retainedFn,"failed replacement preserves the published module");
    await execute(BASE,46,true);

    cpu.jit_clear_cache();e.set_jit_config(7,0);vm.write_memory(Uint8Array.from([0xE9,0xFB,0x0F,0,0]),BASE);put(OTHER,10);await execute(OTHER,10);
    r=force(BASE);begin=writes.length;put(OTHER,11);assert.equal(await complete(r),false);assertNoInstall(begin);
    await execute(OTHER,11);r=force(BASE);await reject(r);assert.equal(e.jit_force_generate_unsafe(BASE),0);
    put(OTHER,12);r=force(BASE);assert.equal(await complete(r),true,"write to secondary dependency permits retry");await execute(BASE,12);

    cpu.jit_clear_cache();put(BASE,13);const saved=await vm.save_state();r=force(BASE);begin=writes.length;
    await vm.restore_state(saved);assert.equal(await complete(r),false);assertNoInstall(begin);
    r=force(BASE);assert.equal(await complete(r),true);

    cpu.jit_clear_cache();put(BASE,14);r=force(BASE);begin=writes.length;
    const originalWasm=cpu.wm;cpu.wm={...originalWasm};
    try { assert.equal(await complete(r),false);assertNoInstall(begin); }
    finally { cpu.wm=originalWasm; }
    assert(e.codegen_finalize_failed(...key(r)));cpu.jit_clear_cache();

    cpu.jit_clear_cache();
    for(let i=0;i<130;i++){const address=0x120000+i*4096;put(address,i);r=force(address);await reject(r);}
    assert.equal(e.jit_publication_stat(2),128,"failure suppression metadata is bounded");
    cpu.jit_clear_cache();assert.equal(e.jit_publication_stat(2),0);
    cpu.jit_clear_cache();assert(e.jit_test_publication_serial(0xFFFFFFFE,0));
    const tickets=[];
    for(let i=0;i<3;i++){put(BASE,20+i);r=force(BASE);tickets.push([r.args[5]>>>0,r.args[6]>>>0]);assert.equal(await complete(r),true);cpu.jit_clear_cache();}
    assert.deepEqual(tickets,[[0xFFFFFFFF,0],[0,1],[1,1]],"u64 ticket crosses low-word wrap without ABA");
    assert(e.jit_test_publication_serial(0xFFFFFFFF,0xFFFFFFFF));put(BASE,99);assert.equal(e.jit_force_generate_unsafe(BASE),0,"ticket exhaustion stops compilation instead of wrapping");
    console.log(`PASS: ${wasm} transactional JIT publication: no stale installation, slot ABA/duplicate rejection, browser/table failures, retry suppression, dependent-page writes, restore and u64 ticket boundaries (${requests.length} controlled instantiations)`);
} finally {
    WebAssembly.instantiate=nativeInstantiate;if(table && oldSet)table.set=oldSet;
    await vm.destroy();
}
