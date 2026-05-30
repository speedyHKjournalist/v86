// Minimal NVIDIA NV20/GeForce3 PCI shell.
//
// This intentionally stays at the PCI + BAR0/BAR1 stub stage. VRAM
// rendering, option ROM POST, PFIFO execution and PGRAPH are later milestones.

// For Types Only
import { CPU } from "../cpu.js";

import { LOG_PCI } from "../const.js";
import { h } from "../lib.js";
import { dbg_log } from "../log.js";

const NV20_VENDOR_ID = 0x10DE;
const NV20_DEVICE_ID_GEFORCE3_TI_500 = 0x0202;

const NV20_DEFAULT_PCI_ID = 0x13 << 3;
const NV20_DEFAULT_MMIO_BASE = 0xF1000000;
const NV20_MMIO_SIZE = 16 * 1024 * 1024;
const NV20_DEFAULT_VRAM_BASE = 0xD0000000;
const NV20_DEFAULT_VRAM_SIZE = 64 * 1024 * 1024;
const NV20_PMC_BOOT_0 = 0x020200A5;
const NV20_PFB_CFG0 = 0x00007FFF;

const NV20_PMC_INTR_PFIFO = 1 << 8;
const NV20_PMC_INTR_PGRAPH = 1 << 12;
const NV20_PMC_INTR_PTIMER = 1 << 20;
const NV20_PMC_INTR_PCRTC = 1 << 24;
const NV20_PMC_INTR_PBUS = 1 << 28;

const NV20_FIFO_CACHE_ENTRY_COUNT = 0x100;
const NV20_VRAM_LOG_BLOCK_SIZE = 1024 * 1024;
const NV20_VRAM_LOG_SAMPLE_WRITES = 0x10000;

function nv20_now_ms()
{
    if(typeof performance !== "undefined" && performance.now)
    {
        return performance.now();
    }

    return Date.now();
}

/**
 * @constructor
 * @param {CPU} cpu
 * @param {Object=} options
 */
export function NV20GeForce(cpu, options)
{
    options = options || {};

    this.name = "geforce-nv20";

    this.pci_id = options.pci_id || NV20_DEFAULT_PCI_ID;

    const mmio_base = options.mmio_base || NV20_DEFAULT_MMIO_BASE;
    const vram_base = options.vram_base || NV20_DEFAULT_VRAM_BASE;
    const vram_size = options.vram_size || NV20_DEFAULT_VRAM_SIZE;

    this.mmio_base = mmio_base;
    this.vram_base = vram_base;
    this.vram_size = vram_size;
    this.vram = new Uint8Array(vram_size);
    this.vram_trace = options.vram_trace !== false;
    this.vram_log_block_size = options.vram_log_block_size || NV20_VRAM_LOG_BLOCK_SIZE;
    this.vram_log_sample_writes = options.vram_log_sample_writes || NV20_VRAM_LOG_SAMPLE_WRITES;
    this.vram_logged_write_blocks = new Set();
    this.vram_write_count = 0;
    this.vram_dirty_min = vram_size;
    this.vram_dirty_max = 0;
    this.mmio_registers = new Map();
    this.mmio_trace = options.mmio_trace !== false;
    this.mmio_trace_all = !!options.mmio_trace_all;
    this.mmio_seen_reads = new Set();
    this.mmio_seen_writes = new Set();

    this.pci = cpu.devices.pci;
    this.pci_config_space = null;
    this.pci_config_space8 = null;

    this.mc_intr_en = 0;
    this.mc_enable = 0;

    this.bus_intr = 0;
    this.bus_intr_en = 0;

    this.fifo_intr = 0;
    this.fifo_intr_en = 0;
    this.fifo_ramht = 0;
    this.fifo_ramfc = 0;
    this.fifo_ramro = 0;
    this.fifo_mode = 0;
    this.fifo_cache1_push1 = 0;
    this.fifo_cache1_put = 0;
    this.fifo_dma_push = 0;
    this.fifo_dma_instance = 0;
    this.fifo_dma_put = 0;
    this.fifo_dma_get = 0;
    this.fifo_ref_cnt = 0;
    this.fifo_pull0 = 0;
    this.fifo_get = 0;
    this.fifo_grctx_instance = 0;
    this.fifo_cache_method = new Uint32Array(NV20_FIFO_CACHE_ENTRY_COUNT);
    this.fifo_cache_data = new Uint32Array(NV20_FIFO_CACHE_ENTRY_COUNT);

    this.timer_intr = 0;
    this.timer_intr_en = 0;
    this.timer_num = 0;
    this.timer_den = 0;
    this.timer_alarm = 0;
    this.timer_base_low = 0;
    this.timer_base_high = 0;
    this.timer_epoch_ms = nv20_now_ms();

    this.straps0_primary_original = options.straps0_primary || 0;
    this.straps0_primary = this.straps0_primary_original;

    this.graph_intr = 0;
    this.graph_intr_en = 0;
    this.graph_nsource = 0;
    this.graph_ctx_switch1 = 0;
    this.graph_ctx_switch2 = 0;
    this.graph_ctx_switch4 = 0;
    this.graph_ctxctl_cur = 0;
    this.graph_status = 0;
    this.graph_trapped_addr = 0;
    this.graph_trapped_data = 0;
    this.graph_notify = 0;
    this.graph_fifo = 0;
    this.graph_channel_ctx_table = 0;

    this.crtc_intr = 0;
    this.crtc_intr_en = 0;
    this.crtc_start = 0;
    this.crtc_config = 0;
    this.crtc_cursor_offset = 0;
    this.crtc_cursor_config = 0;

    this.ramdac_cursor_start = 0;
    this.ramdac_vpll = 0;
    this.ramdac_pll_select = 0;
    this.ramdac_vpll_b = 0;
    this.ramdac_general_control = 0;

    this.pci_space = [
        // 00: vendor/device
        NV20_VENDOR_ID & 0xFF, NV20_VENDOR_ID >> 8,
        NV20_DEVICE_ID_GEFORCE3_TI_500 & 0xFF, NV20_DEVICE_ID_GEFORCE3_TI_500 >> 8,

        // 04: command/status. Keep memory and bus mastering enabled for now,
        // matching v86's existing simple PCI-device convention.
        0x06, 0x00, 0x00, 0x00,

        // 08: revision, prog-if, subclass, class (VGA compatible controller)
        0xA3, 0x00, 0x00, 0x03,
        // 0C: cache line, latency, header type, BIST
        0x00, 0x00, 0x00, 0x00,

        // 10: BAR0, MMIO registers
        mmio_base & 0xFF, mmio_base >> 8 & 0xFF, mmio_base >> 16 & 0xFF, mmio_base >>> 24,
        // 14: BAR1, framebuffer aperture. Bit 3 marks prefetchable memory.
        vram_base & 0xFF | 0x08, vram_base >> 8 & 0xFF, vram_base >> 16 & 0xFF, vram_base >>> 24,
        // 18..27: unused BARs
        0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00,

        // 28: CardBus CIS pointer
        0x00, 0x00, 0x00, 0x00,
        // 2C: subsystem vendor/device, unknown for the generic shell
        0x00, 0x00, 0x00, 0x00,
        // 30: expansion ROM base, disabled until option ROM plumbing exists
        0x00, 0x00, 0x00, 0x00,
        // 34: capabilities pointer
        0x00, 0x00, 0x00, 0x00,
        // 38: reserved
        0x00, 0x00, 0x00, 0x00,
        // 3C: interrupt line, interrupt pin, min grant, max latency
        0x00, 0x01, 0x00, 0x00,
    ];

    this.pci_bars = [
        {
            size: NV20_MMIO_SIZE,
            read8: this.mmio_read8,
            write8: this.mmio_write8,
            read32: this.mmio_read32,
            write32: this.mmio_write32,
            on_remap: function(from, to) { this.mmio_base = to; },
        },
        {
            size: vram_size,
            remappable: true,
            read8: this.vram_read8,
            write8: this.vram_write8,
            read32: this.vram_read32,
            write32: this.vram_write32,
            on_remap: function(from, to) { this.vram_base = to; },
        },
    ];

    this.pci_config_space = this.pci.register_device(this);
    this.pci_config_space8 = new Uint8Array(this.pci_config_space.buffer);
}

NV20GeForce.prototype.vram_offset = function(offset)
{
    offset = offset >>> 0;

    if(offset >= this.vram_size)
    {
        offset %= this.vram_size;
    }

    return offset;
};

NV20GeForce.prototype.vram_read8 = function(offset)
{
    return this.vram[this.vram_offset(offset)];
};

NV20GeForce.prototype.vram_write8 = function(offset, value)
{
    offset = this.vram_offset(offset);
    value &= 0xFF;

    this.vram[offset] = value;
    this.vram_mark_write(offset, 1, value);
};

NV20GeForce.prototype.vram_read32 = function(offset)
{
    offset = this.vram_offset(offset);

    if(offset + 3 < this.vram_size)
    {
        return (this.vram[offset] |
                this.vram[offset + 1] << 8 |
                this.vram[offset + 2] << 16 |
                this.vram[offset + 3] << 24) >>> 0;
    }

    return (this.vram[offset] |
            this.vram[this.vram_offset(offset + 1)] << 8 |
            this.vram[this.vram_offset(offset + 2)] << 16 |
            this.vram[this.vram_offset(offset + 3)] << 24) >>> 0;
};

NV20GeForce.prototype.vram_write32 = function(offset, value)
{
    offset = this.vram_offset(offset);
    value = value >>> 0;

    if(offset + 3 < this.vram_size)
    {
        this.vram[offset] = value & 0xFF;
        this.vram[offset + 1] = value >> 8 & 0xFF;
        this.vram[offset + 2] = value >> 16 & 0xFF;
        this.vram[offset + 3] = value >>> 24;
    }
    else
    {
        this.vram[offset] = value & 0xFF;
        this.vram[this.vram_offset(offset + 1)] = value >> 8 & 0xFF;
        this.vram[this.vram_offset(offset + 2)] = value >> 16 & 0xFF;
        this.vram[this.vram_offset(offset + 3)] = value >>> 24;
    }

    this.vram_mark_write(offset, 4, value);
};

NV20GeForce.prototype.vram_mark_write = function(offset, width, value)
{
    this.vram_write_count++;

    if(offset < this.vram_dirty_min)
    {
        this.vram_dirty_min = offset;
    }

    const end = Math.min(this.vram_size, offset + width);

    if(end > this.vram_dirty_max)
    {
        this.vram_dirty_max = end;
    }

    this.vram_log_write(offset, width, value);
};

NV20GeForce.prototype.vram_log_write = function(offset, width, value)
{
    if(!this.vram_trace)
    {
        return;
    }

    const block = Math.floor(offset / this.vram_log_block_size);
    var reason = "";

    if(!this.vram_logged_write_blocks.has(block))
    {
        this.vram_logged_write_blocks.add(block);
        reason = "first-block";
    }
    else if(this.vram_log_sample_writes && this.vram_write_count % this.vram_log_sample_writes === 0)
    {
        reason = "sample";
    }

    if(!reason)
    {
        return;
    }

    dbg_log(this.name + " vram write " + h(offset >>> 0, 8) +
            " width=" + width +
            " value=" + h(value >>> 0, width === 1 ? 2 : 8) +
            " count=" + this.vram_write_count +
            " " + reason, LOG_PCI);
};

NV20GeForce.prototype.mmio_read8 = function(offset)
{
    return this.mmio_read32(offset & ~3) >>> ((offset & 3) << 3) & 0xFF;
};

NV20GeForce.prototype.mmio_write8 = function(offset, value)
{
    const shift = (offset & 3) << 3;
    const mask = 0xFF << shift;
    const old_value = this.register_read32(offset & ~3).value;
    this.mmio_write32(offset & ~3, old_value & ~mask | (value & 0xFF) << shift);
};

NV20GeForce.prototype.mmio_read32 = function(offset)
{
    offset = offset & (NV20_MMIO_SIZE - 1) & ~3;

    const result = this.register_read32(offset);
    this.mmio_log("read", offset, result.value, result.known);
    return result.value;
};

NV20GeForce.prototype.mmio_write32 = function(offset, value)
{
    offset = offset & (NV20_MMIO_SIZE - 1) & ~3;
    value = value >>> 0;

    const known = this.register_write32(offset, value);
    this.mmio_log("write", offset, value, known);
};

NV20GeForce.prototype.register_read32 = function(offset)
{
    var known = true;
    var value = 0;

    if(offset >= 0x1800 && offset < 0x1900)
    {
        value = this.pci_config_read32(offset - 0x1800);
    }
    else if(offset >= 0x3800 && offset < 0x4000)
    {
        const index = offset - 0x3800 >>> 3;
        value = offset & 4 ? this.fifo_cache_data[index] : this.fifo_cache_method[index];
    }
    else
    {
        switch(offset)
        {
            case 0x000000:
                value = NV20_PMC_BOOT_0;
                break;
            case 0x000100:
                value = this.get_master_interrupt_status();
                break;
            case 0x000140:
                value = this.mc_intr_en;
                break;
            case 0x000200:
                value = this.mc_enable;
                break;

            case 0x001100:
                value = this.bus_intr;
                break;
            case 0x001140:
                value = this.bus_intr_en;
                break;

            case 0x002100:
                value = this.fifo_intr;
                break;
            case 0x002140:
                value = this.fifo_intr_en;
                break;
            case 0x002210:
                value = this.fifo_ramht;
                break;
            case 0x002214:
                value = this.fifo_ramfc;
                break;
            case 0x002218:
                value = this.fifo_ramro;
                break;
            case 0x002400:
                value = 0x10;
                break;
            case 0x002504:
                value = this.fifo_mode;
                break;
            case 0x003204:
                value = this.fifo_cache1_push1;
                break;
            case 0x003210:
                value = this.fifo_cache1_put;
                break;
            case 0x003214:
                value = 0x10;
                break;
            case 0x003220:
                value = this.fifo_dma_push;
                break;
            case 0x00322C:
                value = this.fifo_dma_instance;
                break;
            case 0x003230:
                value = 0x80000000;
                break;
            case 0x003240:
                value = this.fifo_dma_put;
                break;
            case 0x003244:
                value = this.fifo_dma_get;
                break;
            case 0x003248:
                value = this.fifo_ref_cnt;
                break;
            case 0x003250:
                value = this.fifo_pull0 | (this.fifo_dma_get !== this.fifo_dma_put ? 0x100 : 0);
                break;
            case 0x003270:
                value = this.fifo_get;
                break;
            case 0x0032E0:
                value = this.fifo_grctx_instance;
                break;
            case 0x003304:
                value = 1;
                break;

            case 0x009100:
                value = this.timer_intr;
                break;
            case 0x009140:
                value = this.timer_intr_en;
                break;
            case 0x009200:
                value = this.timer_num;
                break;
            case 0x009210:
                value = this.timer_den;
                break;
            case 0x009400:
                value = this.timer_read_low();
                break;
            case 0x009410:
                value = this.timer_read_high();
                break;
            case 0x009420:
                value = this.timer_alarm;
                break;

            case 0x10020C:
                value = this.vram_size;
                break;
            case 0x100320:
                value = NV20_PFB_CFG0;
                break;
            case 0x101000:
                value = this.straps0_primary;
                break;

            case 0x400100:
                value = this.graph_intr;
                break;
            case 0x400108:
                value = this.graph_nsource;
                break;
            case 0x400140:
                value = this.graph_intr_en;
                break;
            case 0x40014C:
                value = this.graph_ctx_switch1;
                break;
            case 0x400150:
                value = this.graph_ctx_switch2;
                break;
            case 0x400158:
                value = this.graph_ctx_switch4;
                break;
            case 0x40032C:
                value = this.graph_ctxctl_cur;
                break;
            case 0x400700:
                value = this.graph_status;
                break;
            case 0x400704:
                value = this.graph_trapped_addr;
                break;
            case 0x400708:
                value = this.graph_trapped_data;
                break;
            case 0x400718:
                value = this.graph_notify;
                break;
            case 0x400720:
                value = this.graph_fifo;
                break;
            case 0x400780:
                value = this.graph_channel_ctx_table;
                break;

            case 0x600100:
                value = this.crtc_intr;
                break;
            case 0x600140:
                value = this.crtc_intr_en;
                break;
            case 0x600800:
                value = this.crtc_start;
                break;
            case 0x600804:
                value = this.crtc_config;
                break;
            case 0x600808:
                value = 0;
                break;
            case 0x60080C:
                value = this.crtc_cursor_offset;
                break;
            case 0x600810:
                value = this.crtc_cursor_config;
                break;
            case 0x600868:
                value = 0;
                break;

            case 0x680300:
                value = this.ramdac_cursor_start;
                break;
            case 0x680404:
                value = 0;
                break;
            case 0x680508:
                value = this.ramdac_vpll;
                break;
            case 0x68050C:
                value = this.ramdac_pll_select;
                break;
            case 0x680578:
                value = this.ramdac_vpll_b;
                break;
            case 0x680600:
                value = this.ramdac_general_control;
                break;
            case 0x680828:
                value = 0;
                break;

            default:
                known = false;
                value = this.mmio_registers.get(offset) || 0;
                break;
        }
    }

    return {
        value: value >>> 0,
        known: known,
    };
};

NV20GeForce.prototype.register_write32 = function(offset, value)
{
    if(offset >= 0x1800 && offset < 0x1900)
    {
        this.pci_config_write32(offset - 0x1800, value);
        return true;
    }

    if(offset >= 0x3800 && offset < 0x4000)
    {
        const index = offset - 0x3800 >>> 3;

        if(offset & 4)
        {
            this.fifo_cache_data[index] = value;
        }
        else
        {
            this.fifo_cache_method[index] = value;
        }

        return true;
    }

    switch(offset)
    {
        case 0x000140:
            this.mc_intr_en = value;
            return true;
        case 0x000200:
            this.mc_enable = value;
            return true;

        case 0x001100:
            this.bus_intr &= ~value;
            return true;
        case 0x001140:
            this.bus_intr_en = value;
            return true;

        case 0x002100:
            this.fifo_intr &= ~value;
            return true;
        case 0x002140:
            this.fifo_intr_en = value;
            return true;
        case 0x002210:
            this.fifo_ramht = value;
            return true;
        case 0x002214:
            this.fifo_ramfc = value;
            return true;
        case 0x002218:
            this.fifo_ramro = value;
            return true;
        case 0x002504:
            this.fifo_mode = value;
            return true;
        case 0x003204:
            this.fifo_cache1_push1 = value;
            return true;
        case 0x003210:
            this.fifo_cache1_put = value;
            return true;
        case 0x003220:
            this.fifo_dma_push = value;
            return true;
        case 0x00322C:
            this.fifo_dma_instance = value;
            return true;
        case 0x003240:
            this.fifo_dma_put = value;
            return true;
        case 0x003244:
            this.fifo_dma_get = value;
            return true;
        case 0x003248:
            this.fifo_ref_cnt = value;
            return true;
        case 0x003250:
            this.fifo_pull0 = value;
            return true;
        case 0x003270:
            this.fifo_get = value;
            return true;
        case 0x0032E0:
            this.fifo_grctx_instance = value;
            return true;

        case 0x009100:
            this.timer_intr &= ~value;
            return true;
        case 0x009140:
            this.timer_intr_en = value;
            return true;
        case 0x009200:
            this.timer_num = value;
            return true;
        case 0x009210:
            this.timer_den = value;
            return true;
        case 0x009400:
            this.timer_base_low = value;
            this.timer_epoch_ms = nv20_now_ms();
            return true;
        case 0x009410:
            this.timer_base_high = value;
            this.timer_epoch_ms = nv20_now_ms();
            return true;
        case 0x009420:
            this.timer_alarm = value;
            return true;

        case 0x101000:
            this.straps0_primary = value & 0x80000000 ? value : this.straps0_primary_original;
            return true;

        case 0x400100:
            this.graph_intr &= ~value;
            return true;
        case 0x400108:
            this.graph_nsource = value;
            return true;
        case 0x400140:
            this.graph_intr_en = value;
            return true;
        case 0x40014C:
            this.graph_ctx_switch1 = value;
            return true;
        case 0x400150:
            this.graph_ctx_switch2 = value;
            return true;
        case 0x400158:
            this.graph_ctx_switch4 = value;
            return true;
        case 0x40032C:
            this.graph_ctxctl_cur = value;
            return true;
        case 0x400700:
            this.graph_status = value;
            return true;
        case 0x400704:
            this.graph_trapped_addr = value;
            return true;
        case 0x400708:
            this.graph_trapped_data = value;
            return true;
        case 0x400718:
            this.graph_notify = value;
            return true;
        case 0x400720:
            this.graph_fifo = value;
            return true;
        case 0x400780:
            this.graph_channel_ctx_table = value;
            return true;

        case 0x600100:
            this.crtc_intr &= ~value;
            return true;
        case 0x600140:
            this.crtc_intr_en = value;
            return true;
        case 0x600800:
            this.crtc_start = value;
            return true;
        case 0x600804:
            this.crtc_config = value;
            return true;
        case 0x60080C:
            this.crtc_cursor_offset = value;
            return true;
        case 0x600810:
            this.crtc_cursor_config = value;
            return true;

        case 0x680300:
            this.ramdac_cursor_start = value;
            return true;
        case 0x680508:
            this.ramdac_vpll = value;
            return true;
        case 0x68050C:
            this.ramdac_pll_select = value;
            return true;
        case 0x680578:
            this.ramdac_vpll_b = value;
            return true;
        case 0x680600:
            this.ramdac_general_control = value;
            return true;
    }

    if(value)
    {
        this.mmio_registers.set(offset, value);
    }
    else
    {
        this.mmio_registers.delete(offset);
    }

    return false;
};

NV20GeForce.prototype.mmio_log = function(kind, offset, value, known)
{
    if(!this.mmio_trace)
    {
        return;
    }

    const seen = kind === "read" ? this.mmio_seen_reads : this.mmio_seen_writes;

    if(!this.mmio_trace_all && seen.has(offset))
    {
        return;
    }

    seen.add(offset);

    dbg_log(this.name + " mmio " + kind + " " + h(offset >>> 0, 6) +
            (kind === "read" ? " -> " : " <- ") + h(value >>> 0, 8) +
            (known ? "" : " (unknown)"), LOG_PCI);
};

NV20GeForce.prototype.get_master_interrupt_status = function()
{
    var value = 0;

    if(this.bus_intr & this.bus_intr_en)
    {
        value |= NV20_PMC_INTR_PBUS;
    }

    if(this.fifo_intr & this.fifo_intr_en)
    {
        value |= NV20_PMC_INTR_PFIFO;
    }

    if(this.timer_intr & this.timer_intr_en)
    {
        value |= NV20_PMC_INTR_PTIMER;
    }

    if(this.graph_intr & this.graph_intr_en)
    {
        value |= NV20_PMC_INTR_PGRAPH;
    }

    if(this.crtc_intr & this.crtc_intr_en)
    {
        value |= NV20_PMC_INTR_PCRTC;
    }

    return value >>> 0;
};

NV20GeForce.prototype.timer_read64 = function()
{
    const elapsed_ms = Math.max(0, nv20_now_ms() - this.timer_epoch_ms);
    return this.timer_base_low + this.timer_base_high * 0x100000000 + elapsed_ms * 1000000;
};

NV20GeForce.prototype.timer_read_low = function()
{
    return this.timer_read64() >>> 0;
};

NV20GeForce.prototype.timer_read_high = function()
{
    return Math.floor(this.timer_read64() / 0x100000000) >>> 0;
};

NV20GeForce.prototype.pci_config_read32 = function(offset)
{
    offset = offset & 0xFC;

    if(!this.pci_config_space8 || offset + 3 >= this.pci_config_space8.length)
    {
        return 0;
    }

    return (this.pci_config_space8[offset] |
            this.pci_config_space8[offset + 1] << 8 |
            this.pci_config_space8[offset + 2] << 16 |
            this.pci_config_space8[offset + 3] << 24) >>> 0;
};

NV20GeForce.prototype.pci_config_write32 = function(offset, value)
{
    offset = offset & 0xFC;

    if(offset < 0x100)
    {
        this.pci.pci_write32(this.pci_id << 8 | offset, value);
    }
};
