const xmmLane = new Set([0x0F50,0x660F50,0x660FD7,0x660FC4,0x660FC5,0x0F2B,0x660F2B,0x660FE7,0xF20FF0]);
// Capability catalogue for the experimental register frontend. Production coverage
// remains Pending until the CPU/runtime path and all required modes are migrated.
const xmmTransfer = new Set([0xf12,0x660f12,0xf16,0x660f16,0xf20f12,0xf30f12,0xf30f16,0xf13,0x660f13,0xf17,0x660f17,0x660f6e,0x660f7e,0xf30f7e,0x660fd6]);
const xmmMoves = new Set([0x0F10,0x0F11,0x660F10,0x660F11,0xF20F10,0xF20F11,0xF30F10,0xF30F11,0x0F28,0x0F29,0x660F28,0x660F29,0x660F6F,0x660F7F,0xF30F6F,0xF30F7F]);
const xmmShuffle = new Set([0x660F70,0xF20F70,0xF30F70,0x0FC6,0x660FC6]);
const xmmImmediate = new Set([0x660F71,0x660F72,0x660F73]);
const xmmInteger = new Set([0x660f60,0x660f61,0x660f62,0x660f68,0x660f69,0x660f6a,0x660f6c,0x660f6d,0x660f63,0x660f67,0x660f6b,0x660fd1,0x660fd2,0x660fd3,0x660fe1,0x660fe2,0x660ff1,0x660ff2,0x660ff3,0xf14,0xf15,0x660f14,0x660f15,0x660ffc,0x660ffd,0x660ffe,0x660fd4,0x660ff8,0x660ff9,0x660ffa,0x660ffb,0x660fec,0x660fed,0x660fdc,0x660fdd,0x660fe8,0x660fe9,0x660fd8,0x660fd9,0x660f64,0x660f65,0x660f66,0x660f74,0x660f75,0x660f76,0x660fda,0x660fde,0x660fea,0x660fee,0x660fe0,0x660fe3,0x660fe4,0x660fe5,0x660ff4,0x660ff6,0x660fd5,0x660ff5,0x660fdb,0x660fdf,0x660feb,0x660fef,0xf54,0xf55,0xf56,0xf57,0x660f54,0x660f55,0x660f56,0x660f57]);
export function experimentalLowering(encoding, operand) {
    const op = encoding.opcode, g = encoding.fixed_g;
    if(xmmImmediate.has(op) && operand === "reg") return "CpuSimdHIR";
    if(op===0x660FF7) return "CpuSimdHIR";
    if(xmmLane.has(op)) return "CpuSimdHIR";
    if(xmmTransfer.has(op)) return "CpuSimdHIR";
    if(xmmShuffle.has(op)) return "CpuSimdHIR";
    if(xmmMoves.has(op) || xmmInteger.has(op)) return "CpuSimdHIR";
    if([0x0F02,0x0F03].includes(op) || op === 0x0F00 && [4,5].includes(g)) return "CpuSelectorQueryHelper";
    if(op === 0x0F00 && [0,1,2,3].includes(g)) return "CpuTaskRegHelper";
    if(op === 0x0F01 && [0,1,2,3,4,6,7].includes(g)) return "CpuDescriptorHelper";
    if(op >= 0x0F20 && op <= 0x0F23) return "CpuControlRegHelper";
    if([0xF4,0xFA,0x0F06,0x0F09,0x0F34,0x0F35].includes(op)) return "CpuSystemHelper";
    if([0x0FA2,0x0F30,0x0F31,0x0F32].includes(op)) return "CpuInfoHelper";
    if([0xF2,0xF3].includes(op >>> 8) && [0xA4,0xA5,0xA6,0xA7,0xAA,0xAB,0xAC,0xAD,0xAE,0xAF,0x6C,0x6D,0x6E,0x6F].includes(op & 255)) return "CpuRepHelper";
    if([0x06,0x07,0x0E,0x16,0x17,0x1E,0x1F,0x9C,0x9D,0x0FA0,0x0FA1,0x0FA8,0x0FA9].includes(op)) return "CpuStackHIR";
    if(op === 0x8C) return operand === "mem" ? "CpuMemoryHIR" : "CpuStateHIR";
    if(op === 0x8E) return "CpuStateHIR";
    if([0xC4,0xC5,0x0FB2,0x0FB4,0x0FB5].includes(op)) return operand === "mem" ? "CpuStateHIR" : "Pending";
    if([0xA4,0xA5,0xA6,0xA7,0xAA,0xAB,0xAC,0xAD,0xAE,0xAF].includes(op)) return "CpuStringHIR";
    if(op >= 0xE4 && op <= 0xE7 || op >= 0xEC && op <= 0xEF || op >= 0x6C && op <= 0x6F) return "CpuIoHIR";
    if(op === 0xD4) return "CpuArithmeticHIR";
    if([0xA0,0xA1,0xA2,0xA3,0xD7].includes(op)) return "CpuMemoryHIR";
    if([0x27,0x2F,0x37,0x3F,0x98,0x99,0x9E,0x9F,0xD5,0xD6,0xFC,0xFD].includes(op)) return "NativeHIR";
    if(op === 0x0FC7 && g === 1 && operand === "mem") return "CpuMemoryHIR";
    if([0x86,0x87,0x0FB0,0x0FB1,0x0FC0,0x0FC1].includes(op))
        return operand === "mem" ? "CpuMemoryHIR" : "NativeHIR";
    if([0x0FA3,0x0FAB,0x0FB3,0x0FBB,0x0FBC,0x0FBD,0xF30FB8].includes(op) || op === 0x0FBA && g >= 4 || op >= 0x0FC8 && op <= 0x0FCF)
        return operand === "mem" ? "CpuMemoryHIR" : "NativeHIR";
    if([0x69,0x6B,0x0FAF].includes(op) || [0xF6,0xF7].includes(op) && g >= 4)
        return operand === "mem" ? "CpuMemoryHIR" : [0xF6,0xF7].includes(op) && g >= 6 ? "CpuArithmeticHIR" : "NativeHIR";
    if([0xC0,0xC1,0xD0,0xD1,0xD2,0xD3,0x0FA4,0x0FA5,0x0FAC,0x0FAD].includes(op))
        return operand === "mem" ? "CpuMemoryHIR" : "NativeHIR";
    if([0xE8,0xC2,0xC3].includes(op) || op === 0xFF && [2,4].includes(g)) return "CpuControlHIR";
    if(op >= 0x50 && op <= 0x61 || op === 0xC8 || op === 0xC9 || op === 0x68 || op === 0x6A || op === 0x8F || op === 0xFF && g === 6) return "CpuStackHIR";
    if(operand === "mem") {
        if(op === 0x8D) return "NativeHIR";
        if(op >= 0x88 && op <= 0x8B || op === 0xC6 || op === 0xC7 ||
            [0x0FB6, 0x0FB7, 0x0FBE, 0x0FBF].includes(op) ||
            op <= 0x3B && (op & 7) <= 3 || [0x80, 0x81, 0x83, 0x84, 0x85].includes(op) ||
            [0xFE, 0xFF].includes(op) && g < 2 || [0xF6, 0xF7].includes(op) && [0,2,3].includes(g) ||
            op >= 0x0F40 && op <= 0x0F4F || op >= 0x0F90 && op <= 0x0F9F) return "CpuMemoryHIR";
        return "Pending";
    }
    if(op >= 0xB0 && op <= 0xBF || op >= 0x88 && op <= 0x8B || op === 0xC6 || op === 0xC7 ||
        op >= 0x90 && op <= 0x97 || op === 0x86 || op === 0x87 ||
        op <= 0x3D && (op & 7) <= 5 || op === 0x80 || op === 0x81 || op === 0x83 ||
        op >= 0x40 && op <= 0x4F || (op === 0xFE || op === 0xFF) && g < 2 ||
        (op === 0xF6 || op === 0xF7) && [0, 2, 3].includes(g) ||
        [0x84, 0x85, 0xA8, 0xA9, 0xF5, 0xF8, 0xF9, 0x0FB6, 0x0FB7, 0x0FBE, 0x0FBF].includes(op) ||
        op >= 0x0F90 && op <= 0x0F9F || op >= 0x0F40 && op <= 0x0F4F) return "NativeHIR";
    if(op >= 0x70 && op <= 0x7F || op >= 0x0F80 && op <= 0x0F8F || op >= 0xE0 && op <= 0xE3 || op === 0xE9 || op === 0xEB) return "TerminalBranchHIR";
    return "Pending";
}

// Suite attribution is evidence navigation, not exhaustive per-form acceptance.
export function experimentalTests(encoding, operand) {
    const op = encoding.opcode, g = encoding.fixed_g;
    const category = experimentalLowering(encoding, operand);
    if(category === "Pending") return [];
    let suite;
    if(category === "CpuSimdHIR") suite = op===0x660FF7 ? "simd_masked" : xmmLane.has(op) ? "simd_lane" : xmmTransfer.has(op) ? "simd_transfer" : xmmMoves.has(op) ? "simd_moves" : xmmImmediate.has(op) ? "simd_immediate" : xmmShuffle.has(op) ? "simd_shuffle" : "simd_integer";
    else if(category === "CpuSelectorQueryHelper") suite = op === 0x0F00 ? "verr" : "selector_query";
    else if(category === "CpuTaskRegHelper") suite = "task_regs";
    else if(category === "CpuDescriptorHelper") suite = "descriptor";
    else if(category === "CpuControlRegHelper") suite = "control_regs";
    else if(category === "CpuSystemHelper") suite = "cpu_system";
    else if(category === "CpuInfoHelper") suite = "cpu_info";
    else if(category === "CpuRepHelper") suite = "rep";
    else if([0x8C,0x8E,0xC4,0xC5,0x0FB2,0x0FB4,0x0FB5].includes(op)) suite = "segments";
    else if(category === "CpuIoHIR") suite = "io";
    else if(category === "CpuStringHIR") suite = "strings";
    else if(op >= 0xE0 && op <= 0xE3) suite = "loops";
    else if([0x27,0x2F,0x37,0x3F,0x98,0x99,0x9E,0x9F,0xA0,0xA1,0xA2,0xA3,0xD4,0xD5,0xD6,0xD7,0xFC,0xFD].includes(op)) suite = "misc";
    else if(op === 0x0FC7 && g === 1) suite = "cmpxchg8b";
    else if([0x86,0x87,0x0FB0,0x0FB1,0x0FC0,0x0FC1].includes(op)) suite = "exchange";
    else if([0x0FA3,0x0FAB,0x0FB3,0x0FBB,0x0FBC,0x0FBD,0xF30FB8].includes(op) || op === 0x0FBA && g >= 4 || op >= 0x0FC8 && op <= 0x0FCF) suite = "bits";
    else if([0x69,0x6B,0x0FAF].includes(op) || [0xF6,0xF7].includes(op) && g >= 4) suite = "multiply";
    else if([0xC0,0xC1,0xD0,0xD1,0xD2,0xD3,0x0FA4,0x0FA5,0x0FAC,0x0FAD].includes(op)) suite = "shifts";
    else if(category === "CpuStackHIR") suite = op === 0xC8 ? "enter" :
        [0x06,0x07,0x0E,0x16,0x17,0x1E,0x1F,0x9C,0x9D,0x0FA0,0x0FA1,0x0FA8,0x0FA9].includes(op) ? "system_stack" : "stack";
    else if(category === "CpuControlHIR") suite = "control";
    else if(category === "CpuMemoryHIR") suite = "memory";
    if(suite) return [`tests/ir/semantics/${suite}.rs`, `tests/ir/differential/${suite}.mjs`];
    return ["tests/ir/semantics/core.rs", category === "TerminalBranchHIR" ? "tests/ir/wasm/run.mjs" : "tests/ir/differential/registers.mjs"];
}
