import assert from "node:assert/strict";
import fs from "node:fs";
const catalogue = JSON.parse(fs.readFileSync("docs/ir-coverage.json"));
assert.equal(new Set(catalogue.forms.map(f => f.key)).size, catalogue.forms.length, "duplicate coverage key");
for(const form of catalogue.forms) {
    const covered = form.experimental_lowering !== "Pending";
    assert(!covered || form.tests.length > 0, `missing suite attribution: ${form.key}`);
    for(const path of form.tests) assert(fs.existsSync(path), `missing coverage suite ${path}`);
}
const branches = catalogue.forms.filter(f => f.experimental_lowering === "TerminalBranchHIR").length;
const pending = catalogue.forms.filter(f => f.lowering === "Pending").length;
const experimental = catalogue.forms.filter(f => f.experimental_lowering === "NativeHIR").length;
const memory = catalogue.forms.filter(f => f.experimental_lowering === "CpuMemoryHIR").length;
const stack = catalogue.forms.filter(f => f.experimental_lowering === "CpuStackHIR").length;
const control = catalogue.forms.filter(f => f.experimental_lowering === "CpuControlHIR").length;
const arithmetic = catalogue.forms.filter(f => f.experimental_lowering === "CpuArithmeticHIR").length;
const state = catalogue.forms.filter(f => f.experimental_lowering === "CpuStateHIR").length;
const strings = catalogue.forms.filter(f => f.experimental_lowering === "CpuStringHIR").length;
const io = catalogue.forms.filter(f => f.experimental_lowering === "CpuIoHIR").length;
const rep = catalogue.forms.filter(f => f.experimental_lowering === "CpuRepHelper").length;
const cpuInfo = catalogue.forms.filter(f => f.experimental_lowering === "CpuInfoHelper").length;
const cpuSystem = catalogue.forms.filter(f => f.experimental_lowering === "CpuSystemHelper").length;
const controlRegs = catalogue.forms.filter(f => f.experimental_lowering === "CpuControlRegHelper").length;
const descriptor = catalogue.forms.filter(f => f.experimental_lowering === "CpuDescriptorHelper").length;
const taskRegs = catalogue.forms.filter(f => f.experimental_lowering === "CpuTaskRegHelper").length;
const simd = catalogue.forms.filter(f => f.experimental_lowering === "CpuSimdHIR").length;
const selectorQuery = catalogue.forms.filter(f => f.experimental_lowering === "CpuSelectorQueryHelper").length;
console.log(`${catalogue.encodings} encodings; ${catalogue.forms.length} forms; ${pending} production Pending; ${experimental} experimental NativeHIR forms; ${memory} experimental CPU memory forms; ${stack} experimental CPU stack forms; ${control} experimental CPU control forms; ${arithmetic} experimental CPU arithmetic forms; ${branches} experimental terminal branch forms; ${state} experimental CPU state forms; ${strings} experimental non-REP CPU string forms; ${io} experimental CPU I/O forms; ${rep} experimental REP helper forms; ${cpuInfo} experimental CPU information helper forms; ${cpuSystem} experimental CPU system helper forms; ${controlRegs} experimental control/debug register helper forms; ${descriptor} experimental descriptor/system-word helper forms; ${taskRegs} experimental task/LDTR helper forms; ${selectorQuery} experimental selector-query helper forms; ${simd} experimental XMM forms`);
if(process.argv.includes("--require-complete")) assert.equal(pending, 0, "IR cannot become the default backend while production forms remain Pending");
