import fs from 'node:fs';
import assert from 'node:assert/strict';
import { V86 } from '../../../build/libv86.mjs';

// Headless CPU/disk diagnostic. Disk writes stay in AsyncXHRBuffer's RAM overlay.
// Run one process per backend so 2 GiB guest memories do not overlap.
const [disk, backend = 'ir', wasm = 'build/v86-ir-runtime.wasm'] = process.argv.slice(2);
assert(disk && ['ir', 'legacy'].includes(backend));
const duration = Number(process.env.IR_BOOT_MS || 30000);
assert(Number.isFinite(duration) && duration >= 1000);
const recording = process.env.IR_BENCH_RECORD === '1';
const target = process.env.IR_BOOT_TARGET || 'time';
assert(['time', 'desktop'].includes(target));
let milestone = null;
let phase = 'bios';
const vm = new V86({
    wasm_path: wasm, jit_backend: backend,
    memory_size: 2048 * 1024 * 1024, vga_memory_size: 16 * 1024 * 1024,
    bios: { url: 'bios/seabios.bin' }, vga_bios: { url: 'bios/vgabios.bin' },
    hda: { url: disk, size: fs.statSync(disk).size, async: true },
    x87_fast_math: true, x87_jit_cache: true,
    v86gl_pci: { maxBatchBytes: 16 * 1024 * 1024 },
    disable_keyboard: true, disable_mouse: true, disable_speaker: true,
    net_device: { type: 'ne2k' }, autostart: false,
});
let started, previous, count, total = 0;
vm.add_listener('screen-set-size', size => {
    const ms = started ? performance.now() - started : 0;
    phase = size.join('x');
    console.log(JSON.stringify({ event: 'screen', backend, ms, size }));
    // This is a reproducible display-mode milestone, not proof of desktop idle.
    if(started && !milestone && size[0] === 800 && size[1] === 600 && size[2] === 32) {
        milestone = {ms, instructions: total + (((vm.get_instruction_counter() >>> 0) - count) >>> 0)};
        console.log(JSON.stringify({event:'milestone', name:'800x600x32', backend, ...milestone}));
    }
});
try {
    await new Promise((resolve, reject) => {
        vm.add_listener('emulator-loaded', resolve);
        vm.add_listener('emulator-error', reject);
    });
    const cpu = vm.v86.cpu, e = cpu.wm.exports;
    if(process.env.IR_HOT_FILTER !== undefined) assert.equal(e.ir_auto_set_hot_filter(Number(process.env.IR_HOT_FILTER)),1);
    if(process.env.IR_CACHE_CAPACITY !== undefined) assert.equal(e.ir_cache_set_capacity(Number(process.env.IR_CACHE_CAPACITY)),1);
    if(process.env.IR_FAST_VALIDATION !== undefined) {
        assert(['0', '1'].includes(process.env.IR_FAST_VALIDATION));
        assert.equal(e.ir_cache_set_fast_validation(Number(process.env.IR_FAST_VALIDATION)),1);
    }
    if(process.env.IR_FUSION !== undefined) {
        assert(['0', '1'].includes(process.env.IR_FUSION));
        assert.equal(e.ir_cache_set_fusion(Number(process.env.IR_FUSION)),1);
    }
    if(process.env.IR_DIAGNOSTICS !== undefined) {
        assert.equal(await vm.configure_ir_diagnostics(Number(process.env.IR_DIAGNOSTICS)), true);
    }
    // Opt-in immutable input corpus; no guest memory reads, never enabled in timing runs.
    if(process.env.IR_CAPTURE_FILE) {
        assert.equal(backend,'ir');assert(e.ir_cache_replay_info);
        fs.writeFileSync(process.env.IR_CAPTURE_FILE,'');
        const publish=cpu.ir_auto_publish;
        cpu.ir_auto_publish=function(id,...args) {
            const get=(g,i,f)=>e.ir_cache_replay_info(id,g,i,f)>>>0;
            const header=Array.from({length:5},(_,f)=>get(0,0,f));
            const sources=Array.from({length:header[0]},(_,i)=>{
                const bytes=Buffer.from(new Uint8Array(e.memory.buffer,get(1,i,2),get(1,i,3))).toString('hex');
                const mappings=Array.from({length:get(1,i,4)},(_,m)=>`${get(4,i*65536+m,0)}:${get(4,i*65536+m,1)}`).join(',');
                return `${get(1,i,0)} ${get(1,i,1)} ${bytes} ${mappings}`;
            });
            const entries=Array.from({length:header[1]},(_,i)=>get(2,i,0)).join(',')||'-';
            const edges=Array.from({length:header[2]},(_,i)=>`${get(3,i,0)}:${get(3,i,1)}`).join(',')||'-';
            fs.appendFileSync(process.env.IR_CAPTURE_FILE,`${header[3]} ${header[4]} ${entries} ${edges} ${sources.join(' ')}\n`);
            return publish.call(this,id,...args);
        };
    }
    if(recording) e.performance_recording_enable(1);
    started = previous = performance.now();
    count = vm.get_instruction_counter() >>> 0;
    const readIr = () => [10,2,19,32].map(field=>e.ir_cache_stat(field)>>>0);
    let priorIr = readIr();
    const totalIr = [0,0,0,0];
    const sampleIr = () => {
        const current = readIr(), delta = current.map((value,i)=>(value-priorIr[i])>>>0);
        delta.forEach((value,i)=>totalIr[i]+=value);priorIr=current;return delta;
    };
    vm.run();
    while(performance.now() - started < duration && !(target === 'desktop' && milestone)) {
        await new Promise(resolve => setTimeout(resolve, target === 'desktop' ? 250 : 5000));
        const now = performance.now(), next = vm.get_instruction_counter() >>> 0;
        const steps = (next - count) >>> 0;
        total += steps;
        const delta = sampleIr();
        console.log(JSON.stringify({ backend, wasm, ms: now - started,
            phase, instructions: total,
            interval_ir: {steps:delta[0], activations:delta[1], full_checks:delta[2], observer_checks:delta[3],
                coverage:steps ? delta[0]/steps : 0,
                activations_per_million:steps ? delta[1]*1e6/steps : 0,
                full_checks_per_million:steps ? delta[2]*1e6/steps : 0,
                validation_attempts_per_million:steps ? (delta[2]+delta[3])*1e6/steps : 0},
            requested_ms: duration, overrun_ms: Math.max(0, now - started - duration),
            mips: steps / (now - previous) / 1000, avg_mips: total / (now - started) / 1000,
            recording, jit: vm.get_jit_info(),
            sync_codegen_ms: recording ? e.performance_recording_get(5) : null,
            sync_codegen_calls: recording ? e.performance_recording_get(7) : null,
        }));
        previous = now; count = next;
    }
    await vm.stop();
    total+=((vm.get_instruction_counter()>>>0)-count)>>>0;sampleIr();
    console.log(JSON.stringify({event:'result', backend, target, completed:target === 'time' || !!milestone,
        ms:performance.now()-started, instructions:total, milestone, jit:vm.get_jit_info(),
        // Accumulate wrapping counters per interval; long boots can exceed 2^32.
        ir_work:{guest_steps:totalIr[0],activations:totalIr[1],full_checks:totalIr[2],observer_checks:totalIr[3]},
        boundary_counters:{entry_aliases:e.ir_cache_stat(30)>>>0,shared_publications:e.ir_cache_stat(31)>>>0,
            observer_rejections:e.ir_cache_stat(33)>>>0,shared_compilations:e.ir_auto_stat(25)>>>0,
            shared_extra_entries:e.ir_auto_stat(26)>>>0}}));
    if(recording) {
        const rows = Array.from({ length: e.performance_recording_hotspot_count() }, (_, i) =>
            Array.from({ length: 10 }, (_, j) => e.performance_recording_hotspot_get(i, j)));
        console.log(JSON.stringify({ event: 'samples', backend, rows }));
        const counts = new Map();
        for(const row of rows) counts.set(row[9], (counts.get(row[9]) || 0) + 1);
        const hot = [...counts].sort((a, b) => b[1] - a[1]).slice(0, 12).map(([pc, samples]) => ({
            pc: pc.toString(16), samples,
            // Physical bytes are useful for early identity-mapped BIOS code only.
            physical_bytes: Buffer.from(cpu.mem8.subarray(pc, pc + 32)).toString('hex'),
        }));
        console.log(JSON.stringify({ event: 'hot', backend, hot }));
    }
} finally {
    await vm.destroy();
}
