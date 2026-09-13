import assert from "node:assert/strict";
import fs from "node:fs";
import {V86} from "../../../build/libv86.mjs";
const vm = new V86({wasm_path: "build/v86-ir-test.wasm", memory_size: 32 << 20,
    disable_keyboard: true, disable_mouse: true, disable_speaker: true,
    net_device: {type: "none"}, autostart: false});
try {
    await new Promise(resolve => vm.add_listener("emulator-loaded", resolve));
    const records = fs.readFileSync("build/ir-decode/legacy-forms.bin");
    const cpu = vm.v86.cpu, compare = cpu.wm.exports.ir_test_compare_analysis, address = 0x300040;
    assert.equal(typeof compare, "function"); assert.equal(records.length % 17, 0);
    const savedIp = cpu.instruction_pointer[0];
    const snapshot = cpu.wm.exports.ir_test_snapshot_length;
    assert.equal(snapshot(address), 15);
    assert.equal(snapshot(0x300FFF), 1);
    assert.equal(snapshot(0xA0000), 0, "MMIO is never snapshotted");
    assert.equal(snapshot(32 << 20), 0, "unallocated physical RAM is never read");
    cpu.mem8[0x300FFF] = 0xB8;
    assert.equal(cpu.wm.exports.ir_test_snapshot_decode(0x300FFF), 2, "missing next page is compile stop");
    assert.equal(cpu.instruction_pointer[0], savedIp, "snapshot/decode does not advance real CPU EIP");
    for(let offset = 0; offset < records.length; offset += 17) {
        const mode = records[offset], length = records[offset + 1], bytes = records.subarray(offset + 2, offset + 17);
        cpu.mem8.set(bytes, address);
        const mask = compare(address, length, mode) >>> 0;
        assert.equal(mask, 0, `analyzer mismatch at record ${offset / 17}: mode=${mode ? 32 : 16}, bytes=${bytes.subarray(0, length).toString("hex")}, mask=${mask.toString(16)}`);
    }
    console.log(`PASS: ${records.length / 17} shared-decoder/legacy-analyzer comparisons: length, prefixes, EA fields, block boundaries, EIP flow, conditions and production adapter`);
} finally { await vm.destroy(); }
