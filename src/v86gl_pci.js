// Experimental v86gl device using the existing modern virtio PCI transport.
// The VGL2/GLWG/D9WG rendering protocols are unchanged. See docs/glbridge.md.
import { VirtIO, VIRTIO_F_VERSION_1 } from "./virtio.js";

const MAGIC = 0x324C4756;
const STATE_MAGIC = 0x56514731;
const ARENA_BYTES = 16 * 1024 * 1024;
const HEADER_BYTES = 32;
const REQUEST_BYTES = 24;
const RESPONSE_BYTES = 16;
const F_SHARED_ARENA = 0;
const REGISTER_ARENA = 1;
const SUBMIT = 2;
const UNREGISTER_ARENA = 3;
const OK = 0;
const INVALID = 1;
const NO_ARENA = 2;
const NO_RENDERER = 3;

/** @constructor */
export function V86GLPCI(cpu, bus, options)
{
    options = options || {};
    this.cpu = cpu;
    this.bus = bus || cpu.bus;
    this.maxBatchBytes = options["maxBatchBytes"] || ARENA_BYTES;
    if(!Number.isInteger(this.maxBatchBytes) || this.maxBatchBytes < HEADER_BYTES ||
        this.maxBatchBytes > ARENA_BYTES)
        throw new Error("v86gl maxBatchBytes must be between 32 bytes and 16 MiB");
    this.onSubmit = options["onSubmit"] || null;
    this.arenaAddress = 0;
    this.arenaBytes = 0;
    this.arenaGeneration = 0;
    this.lastFrameId = 0;
    this.lastBytes = 0;
    this.submitCount = 0;
    this.memoryView = null;
    const port = options["port"] || 0xF100;
    if(port < 0 || port > 0xFC00 || (port & 0xFF))
        throw new Error("v86gl virtio port must be 256-byte aligned and fit four I/O BARs");

    this.virtio = new VirtIO(cpu, {
        name: "virtio-v86gl (experimental)",
        pci_id: 0x13 << 3,
        // Provisional, unallocated local ID. NOT the standard virtio-gpu ID.
        device_id: 0x107F,
        subsystem_device_id: 0x5686,
        on_reset: () => {
            this.release_arena();
            this.lastFrameId = this.lastBytes = this.submitCount = 0;
        },
        common: {
            initial_port: port,
            queues: [{ size_supported: 8, notify_offset: 0 }],
            features: [F_SHARED_ARENA, VIRTIO_F_VERSION_1],
            on_driver_ok: () => {},
        },
        notification: {
            initial_port: port + 0x100,
            single_handler: true,
            handlers: [queue => this.notify(queue)],
        },
        isr_status: { initial_port: port + 0x200 },
        device_specific: {
            initial_port: port + 0x300,
            struct: [
                { bytes: 4, name: "magic", read: () => MAGIC, write: data => {} },
                { bytes: 4, name: "version", read: () => 1, write: data => {} },
                { bytes: 4, name: "arena_bytes", read: () => ARENA_BYTES, write: data => {} },
                { bytes: 4, name: "max_batch_bytes", read: () => this.maxBatchBytes, write: data => {} },
            ],
        },
    });
}

V86GLPCI.prototype.release_arena = function()
{
    ++this.arenaGeneration;
    this.arenaAddress = 0;
    this.arenaBytes = 0;
};

V86GLPCI.prototype.reset = function()
{
    this.virtio.reset();
    this.lastFrameId = this.lastBytes = this.submitCount = 0;
};

V86GLPCI.prototype.valid_range = function(address, bytes)
{
    const end = address + bytes;
    return address > 0 && bytes > 0 && end <= this.cpu.memory_size[0] &&
        (end <= 0xA0000 || address >= 0x100000);
};

V86GLPCI.prototype.notify = function(queue_id)
{
    const virtio = this.virtio;
    const queue = virtio.queues[0];
    if(queue_id !== 0 || (virtio.device_status & 0xCF) !== 15 ||
        !virtio.is_feature_negotiated(F_SHARED_ARENA) ||
        !virtio.is_feature_negotiated(VIRTIO_F_VERSION_1) ||
        !queue.enabled || !queue.is_configured()) return;

    // This protocol has exactly two direct descriptors. Validate and consume
    // them once, without constructing a generic scatter/gather buffer chain.
    if(queue.size < 2 || queue.size > 8 || (queue.size & (queue.size - 1)) ||
        (queue.desc_addr & 15) || (queue.avail_addr & 1) || (queue.used_addr & 3) ||
        !this.valid_range(queue.desc_addr, queue.size * 16) ||
        !this.valid_range(queue.avail_addr, 6 + queue.size * 2) ||
        !this.valid_range(queue.used_addr, 6 + queue.size * 8) ||
        queue.count_requests() > queue.size)
    {
        virtio.needs_reset();
        return;
    }
    const ram = this.cpu.mem8;
    if(!this.memoryView || this.memoryView.buffer !== ram.buffer ||
        this.memoryView.byteOffset !== ram.byteOffset || this.memoryView.byteLength !== ram.byteLength)
        this.memoryView = new DataView(ram.buffer, ram.byteOffset, ram.byteLength);
    const memory = this.memoryView;
    let pending = queue.count_requests();
    while(pending--)
    {
        const head = queue.avail_get_entry(queue.avail_last_idx);
        if(head >= queue.size) { queue.flush_replies(); virtio.needs_reset(); return; }
        const read = queue.desc_addr + head * 16;
        const next = memory.getUint16(read + 14, true);
        if(memory.getUint16(read + 12, true) !== 1 || next >= queue.size || next === head)
        {
            queue.flush_replies();
            virtio.needs_reset();
            return;
        }
        const write = queue.desc_addr + next * 16;
        const request = memory.getUint32(read, true);
        const reply = memory.getUint32(write, true);
        if(memory.getUint16(write + 12, true) !== 2 ||
            memory.getUint32(read + 4, true) || memory.getUint32(write + 4, true) ||
            memory.getUint32(read + 8, true) !== REQUEST_BYTES ||
            memory.getUint32(write + 8, true) !== RESPONSE_BYTES ||
            !this.valid_range(request, REQUEST_BYTES) || !this.valid_range(reply, RESPONSE_BYTES))
        {
            queue.flush_replies();
            virtio.needs_reset();
            return;
        }
        queue.avail_last_idx = (queue.avail_last_idx + 1) & 0xFFFF;
        let result = INVALID;
        try { result = this.handle_request(request); }
        catch(error) { console.error("[virtio-v86gl] request failed", error); }
        // Match CPU.write_blob's invalidation contract without a temporary
        // reply buffer or four separate guest-memory helper calls.
        this.cpu.jit_dirty_cache(reply, reply + RESPONSE_BYTES);
        memory.setUint32(reply, result, true);
        memory.setUint32(reply + 4, this.lastFrameId, true);
        memory.setUint32(reply + 8, this.lastBytes, true);
        memory.setUint32(reply + 12, this.submitCount, true);
        queue.push_reply_id(head, RESPONSE_BYTES);
    }
    queue.flush_replies();
};

V86GLPCI.prototype.handle_request = function(request)
{
    const memory = this.memoryView;
    const op = memory.getUint32(request, true);
    const address = memory.getUint32(request + 4, true);
    const high = memory.getUint32(request + 8, true);
    const length = memory.getUint32(request + 12, true);
    const flags = memory.getUint32(request + 16, true);
    if(memory.getUint32(request + 20, true)) return INVALID;
    if(op === REGISTER_ARENA)
    {
        if(this.arenaBytes || high || flags || length !== ARENA_BYTES ||
            !this.valid_range(address, length)) return INVALID;
        ++this.arenaGeneration;
        this.arenaAddress = address;
        this.arenaBytes = length;
        return OK;
    }
    if(address || high) return INVALID;
    if(op === UNREGISTER_ARENA)
    {
        if(length || flags) return INVALID;
        this.release_arena();
        return OK;
    }
    if(op !== SUBMIT || (flags & ~1)) return INVALID;
    if(!this.arenaBytes) return NO_ARENA;
    if(length < HEADER_BYTES || length > this.arenaBytes || length > this.maxBatchBytes)
        return INVALID;
    const raw = this.cpu.read_blob(this.arenaAddress, length);
    const header = new DataView(raw.buffer, raw.byteOffset, raw.byteLength);
    const command_bytes = header.getUint32(20, true);
    // VGL2 reserved words belong to the existing graphics protocol, not to
    // the virtqueue request. Shipping D3D8/D3D9/DDraw DLLs put D9WG_MAGIC in
    // reserved0; the old PCI transport ignored both words. Preserve that ABI.
    if(header.getUint32(0, true) !== MAGIC || header.getUint32(4, true) !== 1 ||
        command_bytes !== length - HEADER_BYTES || (header.getUint32(8, true) & ~1)) return INVALID;

    const generation = this.arenaGeneration;
    const address_base = this.arenaAddress;
    const arena_bytes = this.arenaBytes;
    const memory_valid = () => generation === this.arenaGeneration && !!this.arenaBytes;
    const event = {
        "frameId": header.getUint32(12, true),
        "flags": header.getUint32(8, true) | flags,
        "commandCount": header.getUint32(16, true),
        "bytes": raw.subarray(HEADER_BYTES),
        "descAddr": address_base,
        "descLen": length,
        "batchAddr": address_base + HEADER_BYTES,
        "responseBase": arena_bytes - 4 * 1024 * 1024 - HEADER_BYTES,
        "submitCount": this.submitCount + 1,
        "handled": false,
        "isMemoryValid": memory_valid,
        "writeGuestMemory": (offset, bytes) => {
            if(!memory_valid()) return;
            if(!Number.isInteger(offset) || offset < 0 || offset + bytes.length > arena_bytes)
                throw new RangeError("v86gl write outside registered arena");
            this.cpu.write_blob(bytes, address_base + offset);
        },
    };
    if(this.onSubmit) this.onSubmit(event);
    this.bus.send("v86gl-pci-frame", event);
    if(!event["handled"]) return NO_RENDERER;
    this.lastFrameId = event["frameId"];
    this.lastBytes = command_bytes;
    ++this.submitCount;
    return OK;
};

V86GLPCI.prototype.get_state = function()
{
    return [STATE_MAGIC, this.virtio, this.arenaAddress, this.arenaBytes,
        this.lastFrameId, this.lastBytes, this.submitCount, 0,
        this.graphics_state_handlers ? this.graphics_state_handlers.save() : undefined];
};

V86GLPCI.prototype.set_state = function(state)
{
    if(!state || state[0] !== STATE_MAGIC)
        throw new Error("Legacy v86gl PCI snapshots cannot use virtio-v86gl; cold-boot the disk with the new driver");
    if(state[3] && (state[3] !== ARENA_BYTES || !this.valid_range(state[2], state[3])))
        throw new Error("Invalid virtio-v86gl arena in snapshot");
    this.release_arena();
    this.virtio.set_state(state[1]);
    this.arenaAddress = state[2];
    this.arenaBytes = state[3];
    this.lastFrameId = state[4];
    this.lastBytes = state[5];
    this.submitCount = state[6];
    if(this.graphics_state_handlers) this.graphics_state_handlers.restore(state[8]);
};
