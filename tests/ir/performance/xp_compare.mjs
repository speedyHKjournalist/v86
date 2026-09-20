// Sequential fresh-VM, same-image paired acceptance; no concurrent benchmark jobs.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
const [disk, prefix='build/ir13-xp-paired'] = process.argv.slice(2);
assert(disk);
const runs = Number(process.env.IR_COMPARE_RUNS || 3);
assert(Number.isInteger(runs) && runs >= 3 && runs <= 10);
const rows=[];
for(let round=0;round<runs;round++) for(const backend of round%2?['legacy','ir']:['ir','legacy']) {
    const file=`${prefix}-${round}-${backend}.jsonl`, fd=fs.openSync(file,'w');
    const child=spawnSync(process.execPath,['tests/ir/performance/xp_boot.mjs',disk,backend],{
        env:{...process.env,IR_BOOT_MS:process.env.IR_BOOT_MS||'180000',IR_BOOT_TARGET:'desktop',IR_DIAGNOSTICS:'0',IR_BENCH_RECORD:'0'},
        stdio:['ignore',fd,fd],timeout:300000,
    });
    fs.closeSync(fd);
    assert.equal(child.status,0,`run failed: ${file}: ${child.error||child.signal||child.status}`);
    const events=fs.readFileSync(file,'utf8').split('\n').filter(l=>l.startsWith('{')).map(l=>JSON.parse(l));
    const result=events.findLast(e=>e.event==='result');
    assert(result?.completed&&result.milestone,`milestone not reached: ${file}`);
    const row={round,backend,file,...result.milestone,mips:result.milestone.instructions/result.milestone.ms/1000};
    rows.push(row);console.log(JSON.stringify(row));
}
const median=a=>a.sort((a,b)=>a-b)[Math.floor(a.length/2)];
const medians=Object.fromEntries(['ir','legacy'].map(backend=>[backend,{
    ms:median(rows.filter(r=>r.backend===backend).map(r=>r.ms)),
    mips:median(rows.filter(r=>r.backend===backend).map(r=>r.mips)),
    instructions:median(rows.filter(r=>r.backend===backend).map(r=>r.instructions)),
}]));
const result={milestone:'first 800x600x32 mode (not desktop idle)',runs,rows,medians,
    time_ratio:medians.ir.ms/medians.legacy.ms,throughput_ratio:medians.ir.mips/medians.legacy.mips,
    pass:medians.ir.ms<=medians.legacy.ms&&medians.ir.mips>=medians.legacy.mips};
fs.writeFileSync(`${prefix}-summary.json`,JSON.stringify(result,null,2)+'\n');
console.log(JSON.stringify(result));

process.exitCode=result.pass?0:1;
