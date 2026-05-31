// Minimal NVIDIA NV20/GeForce3 PCI shell.
//
// This intentionally stays at the PCI + BAR0/BAR1 + framebuffer bridge stage
// with enough PFIFO/RAMIN plumbing for drivers to create channels and submit
// commands. Full PGRAPH execution is a later milestone.

// For Types Only
import { CPU } from "../cpu.js";

import { LOG_PCI, MMAP_BLOCK_SIZE } from "../const.js";
import { h } from "../lib.js";
import { dbg_log } from "../log.js";

const NV20_VENDOR_ID = 0x10DE;
const NV20_DEVICE_ID_GEFORCE3_TI_500 = 0x0202;

const NV20_DEFAULT_PCI_ID = 0x13 << 3;
const NV20_DEFAULT_MMIO_BASE = 0xF1000000;
const NV20_MMIO_SIZE = 16 * 1024 * 1024;
const NV20_DEFAULT_VRAM_BASE = 0xD0000000;
const NV20_DEFAULT_VRAM_SIZE = 64 * 1024 * 1024;
const NV20_DEFAULT_ROM_BASE = 0xFE000000;
const NV20_PRAMIN_BASE = 0x00700000;
const NV20_PRAMIN_SIZE = 1024 * 1024;
const NV20_RAMIN_REVERSE_UNIT = 64;
const NV20_PMC_BOOT_0 = 0x020200A5;
const NV20_PFB_CFG0 = 0x00007FFF;

const NV20_PMC_INTR_PFIFO = 1 << 8;
const NV20_PMC_INTR_PGRAPH = 1 << 12;
const NV20_PMC_INTR_PTIMER = 1 << 20;
const NV20_PMC_INTR_PCRTC = 1 << 24;
const NV20_PMC_INTR_PBUS = 1 << 28;

const NV20_FIFO_CACHE_ENTRY_COUNT = 0x100;
const NV20_FIFO_CACHE_GET_MASK = 0xFF;
const NV20_FIFO_CACHE_EMPTY = 0x10;
const NV20_FIFO_INTR_CACHE_ERROR = 1 << 0;
const NV20_FIFO_INTR_DMA_PUSHER = 1 << 12;
const NV20_FIFO_INTR_DMA_PTE = 1 << 16;
const NV20_FIFO_USER_BASE = 0x800000;
const NV20_FIFO_USER_SIZE = 0x200000;
const NV20_FIFO_USER_CHANNEL_STRIDE = 0x10000;
const NV20_FIFO_USER_SUBCHANNEL_STRIDE = 0x2000;
const NV20_FIFO_DMA_USER_BASE = 0xC00000;
const NV20_FIFO_DMA_USER_SIZE = 0x200000;
const NV20_FIFO_DMA_USER_CHANNEL_STRIDE = 0x2000;
const NV20_FIFO_DMA_PUT = 0x40;
const NV20_FIFO_DMA_GET = 0x44;
const NV20_FIFO_REF = 0x48;
const NV20_FIFO_DMA_PUT_HIGH = 0x4C;
const NV20_FIFO_DMA_CGET = 0x54;
const NV20_FIFO_DMA_MGET = 0x58;
const NV20_FIFO_DMA_MGET_HIGH = 0x5C;
const NV20_FIFO_DMA_GET_HIGH = 0x60;
const NV20_FIFO_DMA_KICK_LIMIT = 0x1000;
const NV20_FIFO_METHOD_LOG_LIMIT = 32;
const NV20_RAMHT_ENTRY_SIZE = 8;
const NV20_RAMFC_NV04_STRIDE = 32;
const NV20_RAMFC_NV10_STRIDE = 64;
const NV20_GRAPH_OBJECT_ENGINE = 1;
const NV20_VRAM_LOG_BLOCK_SIZE = 1024 * 1024;
const NV20_VRAM_LOG_SAMPLE_WRITES = 0x10000;
const NV20_DEFAULT_RENDER_WIDTH = 1024;
const NV20_DEFAULT_RENDER_HEIGHT = 768;
const NV20_DEFAULT_RENDER_BPP = 32;
const NV20_DEFAULT_RENDER_FORMAT = "xrgb8888";

const NV20_PRMCIO_CRTC_INDEX_COLOR = 0x6013D4;
const NV20_PRMCIO_CRTC_DATA_COLOR = 0x6013D5;
const NV20_PRMCIO_CRTC_INDEX_MONO = 0x6013B4;
const NV20_PRMCIO_CRTC_DATA_MONO = 0x6013B5;

const NV20_MIN_RENDER_WIDTH = 320;
const NV20_MIN_RENDER_HEIGHT = 200;
const NV20_MAX_RENDER_WIDTH = 4096;
const NV20_MAX_RENDER_HEIGHT = 4096;

const NV20_MMIO_REGISTER_NAMES = new Map([
    [0x000000, "PMC_BOOT_0"],
    [0x000100, "PMC_INTR_0"],
    [0x000140, "PMC_INTR_EN_0"],
    [0x000200, "PMC_ENABLE"],

    [0x001100, "PBUS_INTR_0"],
    [0x001140, "PBUS_INTR_EN_0"],

    [0x002080, "PFIFO_CACHE_ERROR"],
    [0x002100, "PFIFO_INTR_0"],
    [0x002140, "PFIFO_INTR_EN_0"],
    [0x002200, "PFIFO_CONFIG"],
    [0x002210, "PFIFO_RAMHT"],
    [0x002214, "PFIFO_RAMFC"],
    [0x002218, "PFIFO_RAMRO"],
    [0x002400, "PFIFO_RUNOUT_STATUS"],
    [0x002410, "PFIFO_RUNOUT_PUT"],
    [0x002420, "PFIFO_RUNOUT_GET"],
    [0x002500, "PFIFO_CACHES"],
    [0x002504, "PFIFO_MODE"],
    [0x003200, "PFIFO_CACHE1_PUSH0"],
    [0x003204, "PFIFO_CACHE1_PUSH1"],
    [0x003210, "PFIFO_CACHE1_PUT"],
    [0x003214, "PFIFO_CACHE1_STATUS"],
    [0x003218, "PFIFO_CACHE1_DMA_DCOUNT"],
    [0x003220, "PFIFO_CACHE1_DMA_PUSH"],
    [0x00322C, "PFIFO_CACHE1_DMA_INSTANCE"],
    [0x003230, "PFIFO_CACHE1_DMA_STATE"],
    [0x003240, "PFIFO_CACHE1_DMA_PUT"],
    [0x003244, "PFIFO_CACHE1_DMA_GET"],
    [0x003248, "PFIFO_CACHE1_REF_CNT"],
    [0x003250, "PFIFO_CACHE1_PULL0"],
    [0x003254, "PFIFO_CACHE1_PULL1"],
    [0x003270, "PFIFO_CACHE1_GET"],
    [0x0032E0, "PFIFO_CACHE1_ENGINE"],
    [0x0032E4, "PFIFO_CACHE1_DMA_FETCH"],
    [0x003304, "PFIFO_RUNOUT_STATUS"],

    [0x009100, "PTIMER_INTR_0"],
    [0x009140, "PTIMER_INTR_EN_0"],
    [0x009200, "PTIMER_NUMERATOR"],
    [0x009210, "PTIMER_DENOMINATOR"],
    [0x009400, "PTIMER_TIME_0"],
    [0x009410, "PTIMER_TIME_1"],
    [0x009420, "PTIMER_ALARM_0"],

    [0x0C03C2, "PRMVIO_MISC_WRITE"],
    [0x0C03CC, "PRMVIO_MISC_READ"],

    [0x100000, "PFB_BOOT_0"],
    [0x100200, "PFB_CFG"],
    [0x10020C, "PFB_CSTATUS"],
    [0x100320, "PFB_CFG0"],
    [0x101000, "PEXTDEV_BOOT_0"],

    [0x400100, "PGRAPH_INTR"],
    [0x400108, "PGRAPH_NSOURCE"],
    [0x400140, "PGRAPH_INTR_EN"],
    [0x40014C, "PGRAPH_CTX_SWITCH1"],
    [0x400150, "PGRAPH_CTX_SWITCH2"],
    [0x400158, "PGRAPH_CTX_SWITCH4"],
    [0x40032C, "PGRAPH_CTXCTL_CUR"],
    [0x400700, "PGRAPH_STATUS"],
    [0x400704, "PGRAPH_TRAPPED_ADDR"],
    [0x400708, "PGRAPH_TRAPPED_DATA"],
    [0x400718, "PGRAPH_NOTIFY"],
    [0x40071C, "PGRAPH_NOTIFY_INSTANCE"],
    [0x400720, "PGRAPH_FIFO"],
    [0x400724, "PGRAPH_BPIXEL"],
    [0x400780, "PGRAPH_CHANNEL_CTX_TABLE"],
    [0x400820, "PGRAPH_OFFSET0"],
    [0x400850, "PGRAPH_PITCH0"],

    [0x600100, "PCRTC_INTR_0"],
    [0x600140, "PCRTC_INTR_EN_0"],
    [0x600800, "PCRTC_START"],
    [0x600804, "PCRTC_CONFIG"],
    [0x600808, "PCRTC_RASTER"],
    [0x60080C, "PCRTC_CURSOR_OFFSET"],
    [0x600810, "PCRTC_CURSOR_CONFIG"],
    [0x60081C, "PCRTC_GPIO_EXT"],
    [0x600868, "PCRTC_ENGINE_CTRL"],
    [0x6013B4, "PRMCIO_CRTC_INDEX_MONO"],
    [0x6013B5, "PRMCIO_CRTC_DATA_MONO"],
    [0x6013D4, "PRMCIO_CRTC_INDEX_COLOR"],
    [0x6013D5, "PRMCIO_CRTC_DATA_COLOR"],

    [0x680300, "PRAMDAC_CURSOR_START"],
    [0x680404, "PRAMDAC_FP_TG_CONTROL"],
    [0x680508, "PRAMDAC_VPLL"],
    [0x68050C, "PRAMDAC_PLL_SELECT"],
    [0x680578, "PRAMDAC_VPLL_B"],
    [0x680600, "PRAMDAC_GENERAL_CONTROL"],
    [0x680828, "PRAMDAC_DACCLK"],
]);

function nv20_mmio_register_name(offset)
{
    offset = offset >>> 0;

    const name = NV20_MMIO_REGISTER_NAMES.get(offset);

    if(name)
    {
        return name;
    }

    if(offset >= 0x1800 && offset < 0x1900)
    {
        return "PBUS_PCI_CONFIG[" + h(offset - 0x1800, 2) + "]";
    }

    if(offset >= 0x3800 && offset < 0x4000)
    {
        const index = offset - 0x3800 >>> 3;
        return (offset & 4 ? "PFIFO_CACHE1_DATA" : "PFIFO_CACHE1_METHOD") + "[" + h(index, 2) + "]";
    }

    if(offset >= NV20_PRAMIN_BASE && offset < NV20_PRAMIN_BASE + NV20_PRAMIN_SIZE)
    {
        return "PRAMIN[" + h(offset - NV20_PRAMIN_BASE, 5) + "]";
    }

    if(offset >= 0x800000 && offset < 0xA00000)
    {
        return "PFIFO_USER[" + h(offset - 0x800000, 5) + "]";
    }

    if(offset >= 0xC00000 && offset < 0xE00000)
    {
        return "PFIFO_DMA[" + h(offset - 0xC00000, 5) + "]";
    }

    if((offset >= 0x0C0300 && offset < 0x0C0400) ||
        (offset >= 0x0C2300 && offset < 0x0C2400))
    {
        return "PRMVIO[" + h(offset & 0xFF, 2) + "]";
    }

    if((offset >= 0x601300 && offset < 0x601400) ||
        (offset >= 0x603300 && offset < 0x603400))
    {
        return "PRMCIO[" + h(offset & 0xFF, 2) + "]";
    }

    if((offset >= 0x681300 && offset < 0x681400) ||
        (offset >= 0x683300 && offset < 0x683400))
    {
        return "PRAMDIO[" + h(offset & 0xFF, 2) + "]";
    }

    if(offset < 0x1000)
    {
        return "PMC[" + h(offset, 4) + "]";
    }

    if(offset >= 0x1000 && offset < 0x2000)
    {
        return "PBUS[" + h(offset - 0x1000, 4) + "]";
    }

    if(offset >= 0x2000 && offset < 0x4000)
    {
        return "PFIFO[" + h(offset - 0x2000, 4) + "]";
    }

    if(offset >= 0x9000 && offset < 0xA000)
    {
        return "PTIMER[" + h(offset - 0x9000, 4) + "]";
    }

    if(offset >= 0x100000 && offset < 0x102000)
    {
        return "PFB[" + h(offset - 0x100000, 4) + "]";
    }

    if(offset >= 0x400000 && offset < 0x402000)
    {
        return "PGRAPH[" + h(offset - 0x400000, 4) + "]";
    }

    if(offset >= 0x600000 && offset < 0x602000)
    {
        return "PCRTC[" + h(offset - 0x600000, 4) + "]";
    }

    if(offset >= 0x680000 && offset < 0x682000)
    {
        return "PRAMDAC[" + h(offset - 0x680000, 4) + "]";
    }

    return "";
}

function nv20_render_bytes_per_pixel(bpp)
{
    switch(bpp)
    {
        case 8:
            return 1;
        case 15:
        case 16:
            return 2;
        case 24:
            return 3;
        case 32:
            return 4;
        default:
            return Math.max(1, bpp + 7 >>> 3);
    }
}

function nv20_sane_render_bpp(bpp)
{
    return bpp === 8 || bpp === 15 || bpp === 16 || bpp === 24 || bpp === 32;
}

function nv20_init_default_crtc_regs(regs)
{
    // rivafb uses the current CRTC state if no explicit mode is requested.
    // Seed a conservative 640x480x8 state so probe-only loads still expose a
    // real framebuffer layout instead of the render bridge fallback.
    regs[0x01] = 0x4F; // horizontal display end: (0x4f + 1) * 8 = 640
    regs[0x07] = 0x02; // vertical display bit 8
    regs[0x12] = 0xDF; // vertical display low: 0x1df + 1 = 480
    regs[0x13] = 0x50; // CRTC offset: 0x50 * 8 = 640 bytes/line
    regs[0x28] = 0x01; // pixel depth: 8 bpp
}

function nv20_now_ms()
{
    if(typeof performance !== "undefined" && performance.now)
    {
        return performance.now();
    }

    return Date.now();
}

function nv20_next_power_of_2(value)
{
    value = Math.max(1, value >>> 0);
    value--;
    value |= value >>> 1;
    value |= value >>> 2;
    value |= value >>> 4;
    value |= value >>> 8;
    value |= value >>> 16;

    return value + 1 >>> 0;
}

function nv20_option_rom_bar_size(length)
{
    return Math.max(MMAP_BLOCK_SIZE, nv20_next_power_of_2(length));
}

function nv20_normalize_option_rom(rom)
{
    if(!rom)
    {
        return null;
    }

    if(rom instanceof Uint8Array)
    {
        return rom;
    }

    if(rom instanceof ArrayBuffer)
    {
        return new Uint8Array(rom);
    }

    if(ArrayBuffer.isView(rom))
    {
        return new Uint8Array(rom.buffer, rom.byteOffset, rom.byteLength);
    }

    return null;
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
    this.cpu = cpu;

    this.pci_id = options.pci_id || NV20_DEFAULT_PCI_ID;

    const mmio_base = options.mmio_base || NV20_DEFAULT_MMIO_BASE;
    const vram_base = options.vram_base || NV20_DEFAULT_VRAM_BASE;
    const vram_size = options.vram_size || NV20_DEFAULT_VRAM_SIZE;
    const option_rom = nv20_normalize_option_rom(options.option_rom || options.rom || options.pci_rom);

    this.mmio_base = mmio_base;
    this.vram_base = vram_base;
    this.vram_size = vram_size;
    this.vram = new Uint8Array(vram_size);
    this.option_rom = option_rom;

    if(option_rom)
    {
        this.pci_rom_size = nv20_option_rom_bar_size(option_rom.length);
        this.pci_rom_mmap_size = this.pci_rom_size;
        this.pci_rom_address = options.rom_base || NV20_DEFAULT_ROM_BASE;
        this.pci_rom_enabled = false;

        dbg_log("geforce-nv20 external option rom size=" + option_rom.length +
                " bar-size=" + h(this.pci_rom_size >>> 0, 8), LOG_PCI);

        if(option_rom.length < 2 || option_rom[0] !== 0x55 || option_rom[1] !== 0xAA)
        {
            dbg_log("geforce-nv20 option rom has no 55 AA signature", LOG_PCI);
        }
    }

    this.ramin_flip = vram_size - NV20_RAMIN_REVERSE_UNIT;
    this.vram_trace = options.vram_trace !== false;
    this.vram_log_block_size = options.vram_log_block_size || NV20_VRAM_LOG_BLOCK_SIZE;
    this.vram_log_sample_writes = options.vram_log_sample_writes || NV20_VRAM_LOG_SAMPLE_WRITES;
    this.vram_logged_write_blocks = new Set();
    this.vram_write_count = 0;
    this.vram_dirty_min = vram_size;
    this.vram_dirty_max = 0;
    this.render_auto_detect = options.render_auto_detect !== false;
    this.render_width = options.render_width || NV20_DEFAULT_RENDER_WIDTH;
    this.render_height = options.render_height || NV20_DEFAULT_RENDER_HEIGHT;
    this.render_bpp = options.render_bpp || NV20_DEFAULT_RENDER_BPP;
    this.render_stride = options.render_stride || this.render_width * nv20_render_bytes_per_pixel(this.render_bpp);
    this.render_offset = options.render_offset || 0;
    this.render_format = options.render_format || NV20_DEFAULT_RENDER_FORMAT;
    this.render_frame_size = Math.min(this.vram_size - this.render_offset, this.render_stride * this.render_height);
    this.render_dirty_min = this.render_offset + this.render_frame_size;
    this.render_dirty_max = this.render_offset;
    this.render_active = false;
    this.render_initialized = false;
    this.render_pending = false;
    this.render_update_count = 0;
    this.render_buffer = null;
    this.render_image_data = null;
    this.render_source = "default";
    this.screen = options.screen || cpu.devices.vga && cpu.devices.vga.screen;
    this.bus = options.bus || cpu.devices.vga && cpu.devices.vga.bus;
    this.mmio_registers = new Map();
    this.mmio_trace = options.mmio_trace !== false;
    this.mmio_trace_all = !!options.mmio_trace_all;
    this.mmio_seen_reads = new Set();
    this.mmio_seen_writes = new Set();
    this.fifo_trace = options.fifo_trace !== false;
    this.fifo_log_method_limit = options.fifo_log_method_limit || NV20_FIFO_METHOD_LOG_LIMIT;
    this.fifo_method_log_count = 0;

    this.pci = cpu.devices.pci;
    this.pci_config_space = null;
    this.pci_config_space8 = null;

    this.mc_soft_intr = false;
    this.mc_intr_en = 0;
    this.mc_enable = 0;

    this.bus_intr = 0;
    this.bus_intr_en = 0;

    this.fifo_cache_error = 0;
    this.fifo_intr = 0;
    this.fifo_intr_en = 0;
    this.fifo_config = 0;
    this.fifo_ramht = 0;
    this.fifo_ramfc = 0;
    this.fifo_ramro = 0;
    this.fifo_runout_put = 0;
    this.fifo_runout_get = 0;
    this.fifo_caches = 0;
    this.fifo_mode = 0;
    this.fifo_cache1_push0 = 0;
    this.fifo_cache1_push1 = 0;
    this.fifo_cache1_put = 0;
    this.fifo_dma_push = 0;
    this.fifo_dma_instance = 0;
    this.fifo_dma_dcount = 0;
    this.fifo_dma_state = 0;
    this.fifo_dma_put = 0;
    this.fifo_dma_get = 0;
    this.fifo_ref_cnt = 0;
    this.fifo_pull0 = 0;
    this.fifo_pull1 = 0;
    this.fifo_get = 0;
    this.fifo_engine = 0;
    this.fifo_dma_fetch = 0;
    this.fifo_active_channel = 0;
    this.fifo_channels = [];
    this.fifo_subchannels = new Array(8);
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
    this.graph_bpixel = 0;
    this.graph_channel_ctx_table = 0;
    this.graph_offset0 = 0;
    this.graph_pitch0 = 0;

    this.crtc_intr = 0;
    this.crtc_intr_en = 0;
    this.crtc_start = 0;
    this.crtc_config = 0;
    this.crtc_raster_pos = 0;
    this.crtc_cursor_offset = 0;
    this.crtc_cursor_config = 0;
    this.crtc_gpio_ext = 0;
    this.prmcio_crtc_index = 0;
    this.prmcio_crtc_regs = new Uint8Array(0x100);
    nv20_init_default_crtc_regs(this.prmcio_crtc_regs);

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
        // 30: expansion ROM base, disabled by bit 0 until the guest enables it
        this.pci_rom_address & 0xFF, this.pci_rom_address >> 8 & 0xFF,
        this.pci_rom_address >> 16 & 0xFF, this.pci_rom_address >>> 24,
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

NV20GeForce.prototype.pci_rom_read8 = function(offset)
{
    if(this.option_rom && offset < this.option_rom.length)
    {
        return this.option_rom[offset];
    }

    return 0;
};

NV20GeForce.prototype.pci_rom_read32 = function(offset)
{
    return (this.pci_rom_read8(offset) |
            this.pci_rom_read8(offset + 1) << 8 |
            this.pci_rom_read8(offset + 2) << 16 |
            this.pci_rom_read8(offset + 3) << 24) >>> 0;
};

NV20GeForce.prototype.pci_rom_on_remap = function(from, to, enabled)
{
    dbg_log("geforce-nv20 option rom " + (enabled ? "mapped" : "disabled") +
            " base=" + h(to >>> 0, 8) +
            " size=" + h((this.pci_rom_size || 0) >>> 0, 8) +
            " old=" + h(from >>> 0, 8), LOG_PCI);
};

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

NV20GeForce.prototype.ramin_read32 = function(offset)
{
    offset = offset & (NV20_PRAMIN_SIZE - 1) & ~3;

    return this.vram_read32((offset ^ this.ramin_flip) >>> 0);
};

NV20GeForce.prototype.ramin_write32 = function(offset, value)
{
    offset = offset & (NV20_PRAMIN_SIZE - 1) & ~3;

    this.vram_write32((offset ^ this.ramin_flip) >>> 0, value);
};

NV20GeForce.prototype.fifo_channel = function(chid)
{
    chid &= 0x7F;

    var channel = this.fifo_channels[chid];

    if(channel)
    {
        return channel;
    }

    channel = {
        id: chid,
        dma_put: 0,
        dma_get: 0,
        dma_put_high: 0,
        dma_get_high: 0,
        dma_mget: 0,
        dma_mget_high: 0,
        dma_cget: 0,
        ref: 0,
        dma_instance: 0,
        dma_state: 0x80000000,
        dma_fetch: 0,
        dma_dcount: 0,
        dma_subroutine: 0,
        dma_subroutine_active: false,
        context_loaded: false,
        processing: false,
        pending_method: 0,
        pending_count: 0,
        pending_subchannel: 0,
        pending_non_increasing: false,
        subchannels: new Array(8),
    };

    this.fifo_channels[chid] = channel;
    return channel;
};

NV20GeForce.prototype.fifo_active_channel_state = function()
{
    return this.fifo_channel(this.fifo_active_channel);
};

NV20GeForce.prototype.fifo_set_active_channel = function(chid)
{
    const channel = this.fifo_channel(chid);

    this.fifo_active_channel = channel.id;
    this.fifo_load_channel_context(channel);
    this.fifo_sync_cache1_from_channel(channel);
};

NV20GeForce.prototype.fifo_sync_cache1_from_channel = function(channel)
{
    this.fifo_dma_put = channel.dma_put >>> 0;
    this.fifo_dma_get = channel.dma_get >>> 0;
    this.fifo_ref_cnt = channel.ref >>> 0;
    this.fifo_dma_instance = channel.dma_instance >>> 0;
    this.fifo_dma_dcount = channel.dma_dcount >>> 0;
    this.fifo_dma_state = channel.dma_state >>> 0;
    this.fifo_dma_fetch = channel.dma_fetch >>> 0;
};

NV20GeForce.prototype.fifo_ramht_info = function()
{
    var entry_bits = (this.fifo_ramht >>> 16 & 0xFF) + 9;

    if(entry_bits < 9)
    {
        entry_bits = 9;
    }
    else if(entry_bits > 16)
    {
        entry_bits = 16;
    }

    return {
        base: (this.fifo_ramht & 0xFFFF) << 8 & (NV20_PRAMIN_SIZE - 1),
        entry_bits: entry_bits,
        entries: 1 << entry_bits,
        search: this.fifo_ramht >>> 24 & 0xFF,
    };
};

NV20GeForce.prototype.fifo_ramfc_info = function()
{
    return {
        base: (this.fifo_ramfc & 0xFFFF) << 8 & (NV20_PRAMIN_SIZE - 1),
        stride: this.fifo_ramfc & 0x10000 ? NV20_RAMFC_NV10_STRIDE : NV20_RAMFC_NV04_STRIDE,
    };
};

NV20GeForce.prototype.fifo_ramfc_offset = function(chid)
{
    const info = this.fifo_ramfc_info();
    return info.base + (chid & 0x7F) * info.stride & (NV20_PRAMIN_SIZE - 1);
};

NV20GeForce.prototype.fifo_load_channel_context = function(channel)
{
    if(channel.context_loaded)
    {
        return;
    }

    channel.context_loaded = true;

    if(!this.fifo_ramfc)
    {
        return;
    }

    const info = this.fifo_ramfc_info();
    const offset = this.fifo_ramfc_offset(channel.id);

    channel.dma_put = this.ramin_read32(offset + 0x00);
    channel.dma_get = this.ramin_read32(offset + 0x04);
    channel.ref = this.ramin_read32(offset + 0x08);
    channel.dma_instance = this.ramin_read32(offset + 0x0C) & 0xFFFF;
    channel.dma_dcount = this.ramin_read32(offset + 0x10);
    channel.dma_state = this.ramin_read32(offset + 0x14) || 0x80000000;
    channel.dma_fetch = this.ramin_read32(offset + 0x18);

    if(info.stride >= NV20_RAMFC_NV10_STRIDE)
    {
        this.fifo_engine = this.ramin_read32(offset + 0x20);
    }

    this.fifo_sync_cache1_from_channel(channel);
};

NV20GeForce.prototype.fifo_save_channel_context = function(channel)
{
    if(!this.fifo_ramfc)
    {
        return;
    }

    const info = this.fifo_ramfc_info();
    const offset = this.fifo_ramfc_offset(channel.id);

    this.ramin_write32(offset + 0x00, channel.dma_put);
    this.ramin_write32(offset + 0x04, channel.dma_get);
    this.ramin_write32(offset + 0x08, channel.ref);
    this.ramin_write32(offset + 0x0C, channel.dma_instance);
    this.ramin_write32(offset + 0x10, channel.dma_dcount);
    this.ramin_write32(offset + 0x14, channel.dma_state);
    this.ramin_write32(offset + 0x18, channel.dma_fetch);

    if(info.stride >= NV20_RAMFC_NV10_STRIDE)
    {
        this.ramin_write32(offset + 0x20, this.fifo_engine);
    }
};

NV20GeForce.prototype.fifo_hash_handle = function(handle, chid, entry_bits)
{
    const mask = (1 << entry_bits) - 1;
    var hash = chid & mask;
    var value = handle >>> 0;

    while(value)
    {
        hash ^= value & mask;
        value >>>= entry_bits;
    }

    return hash & mask;
};

NV20GeForce.prototype.fifo_ramht_lookup = function(chid, handle)
{
    handle >>>= 0;
    chid &= 0x7F;

    const info = this.fifo_ramht_info();
    const start = this.fifo_hash_handle(handle, chid, info.entry_bits);
    const entries = info.entries;

    for(var pass = 0; pass < entries; pass++)
    {
        const index = start + pass & (entries - 1);
        const offset = info.base + index * NV20_RAMHT_ENTRY_SIZE & (NV20_PRAMIN_SIZE - 1);
        const entry_handle = this.ramin_read32(offset);
        const context = this.ramin_read32(offset + 4);

        if(entry_handle === 0 && context === 0)
        {
            continue;
        }

        if(entry_handle !== handle)
        {
            continue;
        }

        const context_channel = context >>> 24 & 0x7F;

        if(context_channel && context_channel !== chid)
        {
            continue;
        }

        const instance = (context & 0xFFFF) << 4 & (NV20_PRAMIN_SIZE - 1);
        return {
            handle: handle,
            context: context >>> 0,
            instance: instance >>> 0,
            engine: context >>> 16 & 0x1F,
            channel: context_channel,
            class_id: this.ramin_read32(instance) & 0xFFFF,
            index: index,
        };
    }

    return null;
};

NV20GeForce.prototype.fifo_read_physical32 = function(address)
{
    address >>>= 0;

    const mem8 = this.cpu && this.cpu.mem8;

    if(mem8 && address + 3 < mem8.length)
    {
        return (mem8[address] |
                mem8[address + 1] << 8 |
                mem8[address + 2] << 16 |
                mem8[address + 3] << 24) >>> 0;
    }

    if(address + 3 < this.vram_size)
    {
        return this.vram_read32(address);
    }

    return null;
};

NV20GeForce.prototype.fifo_dma_object = function(channel)
{
    const instance = (channel.dma_instance || this.fifo_dma_instance) & 0xFFFF;

    if(!instance)
    {
        return null;
    }

    const offset = instance << 4 & (NV20_PRAMIN_SIZE - 1);
    const flags = this.ramin_read32(offset);
    const limit = this.ramin_read32(offset + 4);
    const base = this.ramin_read32(offset + 8);

    return {
        instance: instance,
        offset: offset,
        flags: flags,
        limit: limit,
        base: base >>> 0,
        adjust: flags & 0xFFF,
    };
};

NV20GeForce.prototype.fifo_dma_read32 = function(channel, offset)
{
    offset >>>= 0;

    const object = this.fifo_dma_object(channel);
    var value = null;

    if(object)
    {
        if(offset <= object.limit)
        {
            value = this.fifo_read_physical32((object.base + object.adjust + offset) >>> 0);
        }
    }

    if(value === null)
    {
        value = this.fifo_read_physical32(offset);
    }

    if(value === null)
    {
        channel.dma_state = 0x80000000;
        this.fifo_intr |= NV20_FIFO_INTR_DMA_PTE;

        if(this.fifo_trace)
        {
            dbg_log(this.name + " pfifo dma read failed channel=" + channel.id +
                    " get=" + h(offset, 8) +
                    (object ? " instance=" + h(object.instance, 4) +
                              " base=" + h(object.base, 8) +
                              " limit=" + h(object.limit, 8) : ""), LOG_PCI);
        }
    }

    return value;
};

NV20GeForce.prototype.fifo_log_method = function(channel, subchannel, method, data, source)
{
    if(!this.fifo_trace || this.fifo_method_log_count > this.fifo_log_method_limit)
    {
        return;
    }

    if(this.fifo_method_log_count === this.fifo_log_method_limit)
    {
        dbg_log(this.name + " pfifo method log suppressed", LOG_PCI);
        this.fifo_method_log_count++;
        return;
    }

    this.fifo_method_log_count++;
    dbg_log(this.name + " pfifo " + source +
            " channel=" + channel.id +
            " subc=" + subchannel +
            " method=" + h(method >>> 0, 4) +
            " data=" + h(data >>> 0, 8), LOG_PCI);
};

NV20GeForce.prototype.fifo_submit_method = function(chid, subchannel, method, data, source)
{
    const channel = this.fifo_channel(chid);
    subchannel &= 7;
    method = method & 0x1FFC;
    data >>>= 0;

    const index = this.fifo_cache1_put++ & NV20_FIFO_CACHE_GET_MASK;
    this.fifo_cache_method[index] = method | subchannel << 13;
    this.fifo_cache_data[index] = data;
    this.fifo_get = this.fifo_cache1_put & NV20_FIFO_CACHE_GET_MASK;
    this.fifo_pull0 &= ~0x100;

    this.graph_trapped_addr = method | subchannel << 13 | channel.id << 24;
    this.graph_trapped_data = data;

    if(method === 0)
    {
        const object = this.fifo_ramht_lookup(channel.id, data);

        channel.subchannels[subchannel] = object || {
            handle: data,
            context: 0,
            instance: 0,
            engine: NV20_GRAPH_OBJECT_ENGINE,
            class_id: 0,
            missing: true,
        };
        this.fifo_subchannels[subchannel] = channel.subchannels[subchannel];

        if(this.fifo_trace)
        {
            dbg_log(this.name + " pfifo bind object channel=" + channel.id +
                    " subc=" + subchannel +
                    " handle=" + h(data, 8) +
                    (object ? " class=" + h(object.class_id, 4) +
                              " instance=" + h(object.instance, 5) +
                              " engine=" + object.engine : " missing-ramht"), LOG_PCI);
        }

        return;
    }

    this.fifo_log_method(channel, subchannel, method, data, source || "method");
};

NV20GeForce.prototype.fifo_dma_kick = function(channel, source)
{
    if(channel.processing)
    {
        return;
    }

    if(!channel.context_loaded &&
        channel.dma_put === 0 &&
        channel.dma_get === 0 &&
        channel.dma_instance === 0)
    {
        this.fifo_load_channel_context(channel);
    }

    channel.processing = true;

    var budget = NV20_FIFO_DMA_KICK_LIMIT;

    while(budget-- > 0)
    {
        if(channel.pending_count)
        {
            if(channel.dma_get === channel.dma_put)
            {
                break;
            }

            const data = this.fifo_dma_read32(channel, channel.dma_get);

            if(data === null)
            {
                break;
            }

            channel.dma_get = channel.dma_get + 4 >>> 0;
            this.fifo_submit_method(channel.id, channel.pending_subchannel,
                                    channel.pending_method, data, "dma");

            if(!channel.pending_non_increasing)
            {
                channel.pending_method = channel.pending_method + 4 & 0x1FFC;
            }

            channel.pending_count--;
            continue;
        }

        if(channel.dma_get === channel.dma_put)
        {
            break;
        }

        const header = this.fifo_dma_read32(channel, channel.dma_get);

        if(header === null)
        {
            break;
        }

        channel.dma_get = channel.dma_get + 4 >>> 0;

        if(header === 0x00020000)
        {
            if(channel.dma_subroutine_active)
            {
                channel.dma_get = channel.dma_subroutine >>> 0;
                channel.dma_subroutine_active = false;
            }
            continue;
        }

        if((header & 0xE0000003) === 0x20000000)
        {
            channel.dma_get = header & 0x1FFFFFFC;
            continue;
        }

        if((header & 0xE0000003) === 0x00000002)
        {
            channel.dma_subroutine = channel.dma_get;
            channel.dma_subroutine_active = true;
            channel.dma_get = header & 0x1FFFFFFC;
            continue;
        }

        if((header & 0xE0000003) === 0x00000000 ||
            (header & 0xE0000003) === 0x40000000)
        {
            channel.pending_non_increasing = !!(header & 0x40000000);
            channel.pending_count = header >>> 18 & 0x7FF;
            channel.pending_subchannel = header >>> 13 & 7;
            channel.pending_method = header & 0x1FFC;
            channel.dma_dcount = channel.pending_count;
            continue;
        }

        channel.dma_state = 0x80000000;
        this.fifo_cache_error = header >>> 0;
        this.fifo_intr |= NV20_FIFO_INTR_CACHE_ERROR | NV20_FIFO_INTR_DMA_PUSHER;

        if(this.fifo_trace)
        {
            dbg_log(this.name + " pfifo unsupported dma header channel=" + channel.id +
                    " header=" + h(header >>> 0, 8) +
                    " get=" + h(channel.dma_get >>> 0, 8) +
                    " source=" + source, LOG_PCI);
        }
        break;
    }

    channel.processing = false;
    channel.dma_dcount = channel.pending_count >>> 0;
    channel.dma_state = channel.pending_count ? 0 : 0x80000000;
    this.fifo_sync_cache1_from_channel(channel);
    this.fifo_save_channel_context(channel);
};

NV20GeForce.prototype.fifo_dma_user_read32 = function(offset)
{
    const rel = offset - NV20_FIFO_DMA_USER_BASE >>> 0;
    const channel = this.fifo_channel(rel / NV20_FIFO_DMA_USER_CHANNEL_STRIDE | 0);
    const reg = rel & (NV20_FIFO_DMA_USER_CHANNEL_STRIDE - 1);

    switch(reg)
    {
        case NV20_FIFO_DMA_PUT:
            return channel.dma_put;
        case NV20_FIFO_DMA_GET:
            return channel.dma_get;
        case NV20_FIFO_REF:
            return channel.ref;
        case NV20_FIFO_DMA_PUT_HIGH:
            return channel.dma_put_high;
        case NV20_FIFO_DMA_CGET:
            return channel.dma_cget;
        case NV20_FIFO_DMA_MGET:
            return channel.dma_mget;
        case NV20_FIFO_DMA_MGET_HIGH:
            return channel.dma_mget_high;
        case NV20_FIFO_DMA_GET_HIGH:
            return channel.dma_get_high;
        default:
            return 0;
    }
};

NV20GeForce.prototype.fifo_dma_user_write32 = function(offset, value)
{
    const rel = offset - NV20_FIFO_DMA_USER_BASE >>> 0;
    const channel = this.fifo_channel(rel / NV20_FIFO_DMA_USER_CHANNEL_STRIDE | 0);
    const reg = rel & (NV20_FIFO_DMA_USER_CHANNEL_STRIDE - 1);

    this.fifo_load_channel_context(channel);

    switch(reg)
    {
        case NV20_FIFO_DMA_PUT:
            channel.dma_put = value >>> 0;
            this.fifo_set_active_channel(channel.id);
            this.fifo_dma_kick(channel, "user-put");
            return true;
        case NV20_FIFO_DMA_GET:
            channel.dma_get = value >>> 0;
            this.fifo_set_active_channel(channel.id);
            this.fifo_sync_cache1_from_channel(channel);
            return true;
        case NV20_FIFO_REF:
            channel.ref = value >>> 0;
            this.fifo_sync_cache1_from_channel(channel);
            return true;
        case NV20_FIFO_DMA_PUT_HIGH:
            channel.dma_put_high = value >>> 0;
            return true;
        case NV20_FIFO_DMA_CGET:
            channel.dma_cget = value >>> 0;
            return true;
        case NV20_FIFO_DMA_MGET:
            channel.dma_mget = value >>> 0;
            return true;
        case NV20_FIFO_DMA_MGET_HIGH:
            channel.dma_mget_high = value >>> 0;
            return true;
        case NV20_FIFO_DMA_GET_HIGH:
            channel.dma_get_high = value >>> 0;
            return true;
        default:
            return true;
    }
};

NV20GeForce.prototype.fifo_pio_user_read32 = function(offset)
{
    return 0xFFFFFFFF;
};

NV20GeForce.prototype.fifo_pio_user_write32 = function(offset, value)
{
    const rel = offset - NV20_FIFO_USER_BASE >>> 0;
    const chid = rel / NV20_FIFO_USER_CHANNEL_STRIDE | 0;
    const channel_offset = rel & (NV20_FIFO_USER_CHANNEL_STRIDE - 1);
    const subchannel = channel_offset / NV20_FIFO_USER_SUBCHANNEL_STRIDE | 0;
    const method = channel_offset & (NV20_FIFO_USER_SUBCHANNEL_STRIDE - 1) & 0x1FFC;

    this.fifo_set_active_channel(chid);
    this.fifo_submit_method(chid, subchannel, method, value, "pio");
    return true;
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

    const render_start = this.render_offset;
    const render_end = render_start + this.render_frame_size;

    if(end > render_start && offset < render_end)
    {
        const dirty_start = Math.max(offset, render_start);
        const dirty_end = Math.min(end, render_end);

        if(dirty_start < this.render_dirty_min)
        {
            this.render_dirty_min = dirty_start;
        }

        if(dirty_end > this.render_dirty_max)
        {
            this.render_dirty_max = dirty_end;
        }

        this.activate_rendering();
        this.schedule_render();
    }
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

NV20GeForce.prototype.set_render_mode = function(width, height, bpp, stride, offset, source)
{
    width = width >>> 0;
    height = height >>> 0;
    bpp = bpp >>> 0;
    offset = offset >>> 0;

    if(!nv20_sane_render_bpp(bpp) ||
        width < NV20_MIN_RENDER_WIDTH || width > NV20_MAX_RENDER_WIDTH ||
        height < NV20_MIN_RENDER_HEIGHT || height > NV20_MAX_RENDER_HEIGHT ||
        offset >= this.vram_size)
    {
        return false;
    }

    const bytes_per_pixel = nv20_render_bytes_per_pixel(bpp);
    const min_stride = width * bytes_per_pixel;
    stride = (stride >>> 0) || min_stride;

    if(stride < min_stride)
    {
        stride = min_stride;
    }

    const max_height = (this.vram_size - offset) / stride | 0;

    if(!max_height)
    {
        return false;
    }

    if(height > max_height)
    {
        height = max_height;
    }

    const frame_size = stride * height;
    const changed =
        this.render_width !== width ||
        this.render_height !== height ||
        this.render_bpp !== bpp ||
        this.render_stride !== stride ||
        this.render_offset !== offset;

    if(!changed)
    {
        return true;
    }

    this.render_width = width;
    this.render_height = height;
    this.render_bpp = bpp;
    this.render_stride = stride;
    this.render_offset = offset;
    this.render_frame_size = frame_size;
    this.render_dirty_min = offset;
    this.render_dirty_max = offset + frame_size;
    this.render_initialized = false;
    this.render_buffer = null;
    this.render_image_data = null;
    this.render_source = source || "registers";

    if(this.render_active)
    {
        this.screen.set_size_graphical(width, height, width, height);

        if(this.bus)
        {
            this.bus.send("screen-set-size", [width, height, bpp]);
        }

        this.schedule_render();
    }

    dbg_log(this.name + " render bridge mode " +
            width + "x" + height + "x" + bpp +
            " stride=" + h(stride >>> 0, 8) +
            " offset=" + h(offset >>> 0, 8) +
            " source=" + this.render_source, LOG_PCI);
    return true;
};

NV20GeForce.prototype.update_render_mode_from_crtc = function(source)
{
    if(!this.render_auto_detect)
    {
        return;
    }

    const crtc = this.prmcio_crtc_regs;

    const horizontal_display_chars = (crtc[0x01] | (crtc[0x2D] & 0x02) << 7) + 1;
    const width = horizontal_display_chars * 8;
    const vertical_display =
        crtc[0x12] |
        (crtc[0x07] & 0x02) << 7 |
        (crtc[0x07] & 0x40) << 3 |
        (crtc[0x25] & 0x02) << 9 |
        (crtc[0x41] & 0x04) << 9;
    const height = vertical_display + 1;
    const pixel_mode = crtc[0x28] & 3;
    const row_offset = crtc[0x13] | (crtc[0x19] & 0xE0) << 3;

    if(!pixel_mode || !row_offset)
    {
        return;
    }

    const bpp = pixel_mode === 1 ? 8 : pixel_mode === 2 ? 16 : pixel_mode === 3 ? 32 : this.render_bpp;
    const bytes_per_pixel = nv20_render_bytes_per_pixel(bpp);
    var stride = row_offset * 8;

    if(stride < width * bytes_per_pixel)
    {
        stride = width * bytes_per_pixel;
    }

    this.set_render_mode(width, height, bpp, stride, this.crtc_start, source);
};

NV20GeForce.prototype.activate_rendering = function()
{
    if(this.render_active)
    {
        return true;
    }

    if(!this.screen || !this.screen.set_mode || !this.screen.set_size_graphical || !this.screen.update_buffer)
    {
        return false;
    }

    this.screen.set_mode(true);
    this.screen.set_size_graphical(this.render_width, this.render_height, this.render_width, this.render_height);

    if(this.bus)
    {
        this.bus.send("screen-set-size", [this.render_width, this.render_height, this.render_bpp]);
    }

    this.render_active = true;
    dbg_log(this.name + " render bridge enabled " +
            this.render_width + "x" + this.render_height + "x" + this.render_bpp +
            " stride=" + h(this.render_stride >>> 0, 8) +
            " offset=" + h(this.render_offset >>> 0, 8) +
            " format=" + this.render_format, LOG_PCI);
    return true;
};

NV20GeForce.prototype.schedule_render = function()
{
    if(!this.render_active || this.render_pending)
    {
        return;
    }

    this.render_pending = true;

    const render = () =>
    {
        this.render_pending = false;
        this.screen_fill_buffer();
    };

    if(typeof requestAnimationFrame !== "undefined")
    {
        requestAnimationFrame(render);
    }
    else if(typeof setTimeout !== "undefined")
    {
        setTimeout(render, 0);
    }
    else
    {
        render();
    }
};

NV20GeForce.prototype.ensure_render_buffer = function()
{
    if(this.render_initialized)
    {
        return true;
    }

    if(typeof ImageData === "undefined")
    {
        return false;
    }

    this.render_buffer = new Uint8ClampedArray(this.render_width * this.render_height * 4);
    this.render_image_data = new ImageData(this.render_buffer, this.render_width, this.render_height);
    this.render_initialized = true;
    return true;
};

NV20GeForce.prototype.screen_fill_buffer = function()
{
    if(!this.render_active)
    {
        return false;
    }

    if(this.render_dirty_min >= this.render_dirty_max)
    {
        return true;
    }

    if(!this.ensure_render_buffer())
    {
        this.render_dirty_min = this.render_offset + this.render_frame_size;
        this.render_dirty_max = this.render_offset;
        return true;
    }

    const stride = this.render_stride;
    const dirty_min = Math.max(0, this.render_dirty_min - this.render_offset);
    const dirty_max = Math.min(this.render_frame_size, this.render_dirty_max - this.render_offset);
    const min_y = Math.max(0, Math.min(this.render_height, dirty_min / stride | 0));
    const max_y = Math.max(min_y, Math.min(this.render_height, (dirty_max + stride - 1) / stride | 0));

    if(min_y < max_y)
    {
        this.render_rows(min_y, max_y);
        this.screen.update_buffer([{
            image_data: this.render_image_data,
            screen_x: 0,
            screen_y: min_y,
            buffer_x: 0,
            buffer_y: min_y,
            buffer_width: this.render_width,
            buffer_height: max_y - min_y,
        }]);
        this.render_update_count++;

        if(this.render_update_count === 1)
        {
            dbg_log(this.name + " render bridge frame update y=" + min_y +
                    " rows=" + (max_y - min_y), LOG_PCI);
        }
    }

    this.render_dirty_min = this.render_offset + this.render_frame_size;
    this.render_dirty_max = this.render_offset;
    return true;
};

NV20GeForce.prototype.render_rows = function(min_y, max_y)
{
    const src = this.vram;
    const dst = this.render_buffer;

    if(this.render_bpp === 8)
    {
        for(var y = min_y; y < max_y; y++)
        {
            var src_i = this.render_offset + y * this.render_stride;
            var dst_i = y * this.render_width * 4;

            for(var x = 0; x < this.render_width; x++, src_i++, dst_i += 4)
            {
                const color = src[src_i];
                dst[dst_i] = color;
                dst[dst_i + 1] = color;
                dst[dst_i + 2] = color;
                dst[dst_i + 3] = 0xFF;
            }
        }
    }
    else if(this.render_bpp === 15 || this.render_bpp === 16)
    {
        const is_555 = this.render_bpp === 15;

        for(var y = min_y; y < max_y; y++)
        {
            var src_i = this.render_offset + y * this.render_stride;
            var dst_i = y * this.render_width * 4;

            for(var x = 0; x < this.render_width; x++, src_i += 2, dst_i += 4)
            {
                const pixel = src[src_i] | src[src_i + 1] << 8;

                if(is_555)
                {
                    dst[dst_i] = (pixel >> 10 & 0x1F) * 0xFF / 0x1F | 0;
                    dst[dst_i + 1] = (pixel >> 5 & 0x1F) * 0xFF / 0x1F | 0;
                    dst[dst_i + 2] = (pixel & 0x1F) * 0xFF / 0x1F | 0;
                }
                else
                {
                    dst[dst_i] = (pixel >> 11 & 0x1F) * 0xFF / 0x1F | 0;
                    dst[dst_i + 1] = (pixel >> 5 & 0x3F) * 0xFF / 0x3F | 0;
                    dst[dst_i + 2] = (pixel & 0x1F) * 0xFF / 0x1F | 0;
                }

                dst[dst_i + 3] = 0xFF;
            }
        }
    }
    else if(this.render_bpp === 24)
    {
        for(var y = min_y; y < max_y; y++)
        {
            var src_i = this.render_offset + y * this.render_stride;
            var dst_i = y * this.render_width * 4;

            for(var x = 0; x < this.render_width; x++, src_i += 3, dst_i += 4)
            {
                dst[dst_i] = src[src_i + 2];
                dst[dst_i + 1] = src[src_i + 1];
                dst[dst_i + 2] = src[src_i];
                dst[dst_i + 3] = 0xFF;
            }
        }
    }
    else
    {
        for(var y = min_y; y < max_y; y++)
        {
            var src_i = this.render_offset + y * this.render_stride;
            var dst_i = y * this.render_width * 4;

            if(this.render_format === "rgba8888")
            {
                for(var x = 0; x < this.render_width; x++, src_i += 4, dst_i += 4)
                {
                    dst[dst_i] = src[src_i];
                    dst[dst_i + 1] = src[src_i + 1];
                    dst[dst_i + 2] = src[src_i + 2];
                    dst[dst_i + 3] = 0xFF;
                }
            }
            else if(this.render_format === "xbgr8888")
            {
                for(var x = 0; x < this.render_width; x++, src_i += 4, dst_i += 4)
                {
                    dst[dst_i] = src[src_i + 1];
                    dst[dst_i + 1] = src[src_i + 2];
                    dst[dst_i + 2] = src[src_i + 3];
                    dst[dst_i + 3] = 0xFF;
                }
            }
            else
            {
                // Linux fbdev's common XRGB8888 layout has memory bytes B, G, R, X.
                for(var x = 0; x < this.render_width; x++, src_i += 4, dst_i += 4)
                {
                    dst[dst_i] = src[src_i + 2];
                    dst[dst_i + 1] = src[src_i + 1];
                    dst[dst_i + 2] = src[src_i];
                    dst[dst_i + 3] = 0xFF;
                }
            }
        }
    }
};

NV20GeForce.prototype.prmcio_read8 = function(offset)
{
    offset = offset >>> 0;

    if(offset === NV20_PRMCIO_CRTC_INDEX_COLOR || offset === NV20_PRMCIO_CRTC_INDEX_MONO)
    {
        return this.prmcio_crtc_index;
    }

    if(offset === NV20_PRMCIO_CRTC_DATA_COLOR || offset === NV20_PRMCIO_CRTC_DATA_MONO)
    {
        return this.prmcio_crtc_regs[this.prmcio_crtc_index];
    }

    return -1;
};

NV20GeForce.prototype.prmcio_write8 = function(offset, value)
{
    offset = offset >>> 0;
    value &= 0xFF;

    if(offset === NV20_PRMCIO_CRTC_INDEX_COLOR || offset === NV20_PRMCIO_CRTC_INDEX_MONO)
    {
        this.prmcio_crtc_index = value;
        this.update_render_mode_from_crtc("crtc-index[" + h(this.prmcio_crtc_index, 2) + "]");
        return true;
    }

    if(offset === NV20_PRMCIO_CRTC_DATA_COLOR || offset === NV20_PRMCIO_CRTC_DATA_MONO)
    {
        this.prmcio_crtc_regs[this.prmcio_crtc_index] = value;
        this.update_render_mode_from_crtc("crtc[" + h(this.prmcio_crtc_index, 2) + "]");
        return true;
    }

    return false;
};

NV20GeForce.prototype.prmcio_read32 = function(offset)
{
    const low = this.prmcio_read8(offset);

    if(low < 0)
    {
        return -1;
    }

    const high = this.prmcio_read8(offset + 1);
    return low | (high < 0 ? 0 : high << 8);
};

NV20GeForce.prototype.prmcio_write32 = function(offset, value)
{
    if(!this.prmcio_write8(offset, value))
    {
        return false;
    }

    // Packed index/data writes are valid, but a plain 32-bit write of only an
    // index often has zero in the upper bytes. Avoid turning that into data.
    if(value & 0xFFFFFF00)
    {
        this.prmcio_write8(offset + 1, value >>> 8);
    }

    return true;
};

NV20GeForce.prototype.mmio_read8 = function(offset)
{
    const value = this.prmcio_read8(offset);

    if(value >= 0)
    {
        return value;
    }

    return this.mmio_read32(offset & ~3) >>> ((offset & 3) << 3) & 0xFF;
};

NV20GeForce.prototype.mmio_write8 = function(offset, value)
{
    if(this.prmcio_write8(offset, value))
    {
        this.mmio_log("write", offset & ~3, this.register_read32(offset & ~3).value, true);
        return;
    }

    const shift = (offset & 3) << 3;
    const mask = 0xFF << shift;
    const old_value = this.register_read32(offset & ~3).value;
    this.mmio_write32(offset & ~3, old_value & ~mask | (value & 0xFF) << shift);
};

NV20GeForce.prototype.mmio_read32 = function(offset)
{
    offset = offset & (NV20_MMIO_SIZE - 1) & ~3;

    const prmcio_value = this.prmcio_read32(offset);

    if(prmcio_value >= 0)
    {
        this.mmio_log("read", offset, prmcio_value, true);
        return prmcio_value;
    }

    const result = this.register_read32(offset);
    this.mmio_log("read", offset, result.value, result.known);
    return result.value;
};

NV20GeForce.prototype.mmio_write32 = function(offset, value)
{
    offset = offset & (NV20_MMIO_SIZE - 1) & ~3;
    value = value >>> 0;

    if(this.prmcio_write32(offset, value))
    {
        this.mmio_log("write", offset, this.register_read32(offset).value, true);
        return;
    }

    const known = this.register_write32(offset, value);
    this.mmio_log("write", offset, value, known);
};

NV20GeForce.prototype.register_read32 = function(offset)
{
    var known = true;
    var value = 0;

    if(offset >= NV20_PRAMIN_BASE && offset < NV20_PRAMIN_BASE + NV20_PRAMIN_SIZE)
    {
        value = this.ramin_read32(offset - NV20_PRAMIN_BASE);
    }
    else if(offset >= 0x1800 && offset < 0x1900)
    {
        value = this.pci_config_read32(offset - 0x1800);
    }
    else if(offset >= 0x3800 && offset < 0x4000)
    {
        const index = offset - 0x3800 >>> 3;
        value = offset & 4 ? this.fifo_cache_data[index] : this.fifo_cache_method[index];
    }
    else if(offset >= NV20_FIFO_USER_BASE && offset < NV20_FIFO_USER_BASE + NV20_FIFO_USER_SIZE)
    {
        value = this.fifo_pio_user_read32(offset);
    }
    else if(offset >= NV20_FIFO_DMA_USER_BASE && offset < NV20_FIFO_DMA_USER_BASE + NV20_FIFO_DMA_USER_SIZE)
    {
        value = this.fifo_dma_user_read32(offset);
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
                if(this.mc_soft_intr)
                {
                    value |= 0x80000000;
                }
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

            case 0x002080:
                value = this.fifo_cache_error;
                break;
            case 0x002100:
                value = this.fifo_intr;
                break;
            case 0x002140:
                value = this.fifo_intr_en;
                break;
            case 0x002200:
                value = this.fifo_config;
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
                value = (this.fifo_get & NV20_FIFO_CACHE_GET_MASK) ===
                    (this.fifo_cache1_put & NV20_FIFO_CACHE_GET_MASK) ? NV20_FIFO_CACHE_EMPTY : 0;
                break;
            case 0x002410:
                value = this.fifo_runout_put;
                break;
            case 0x002420:
                value = this.fifo_runout_get;
                break;
            case 0x002500:
                value = this.fifo_caches;
                break;
            case 0x002504:
                value = this.fifo_mode;
                break;
            case 0x003200:
                value = this.fifo_cache1_push0;
                break;
            case 0x003204:
                value = this.fifo_cache1_push1;
                break;
            case 0x003210:
                value = this.fifo_cache1_put;
                break;
            case 0x003214:
                value = (this.fifo_get & NV20_FIFO_CACHE_GET_MASK) ===
                    (this.fifo_cache1_put & NV20_FIFO_CACHE_GET_MASK) ? NV20_FIFO_CACHE_EMPTY : 0;
                break;
            case 0x003218:
                value = this.fifo_dma_dcount;
                break;
            case 0x003220:
                value = this.fifo_dma_push;
                break;
            case 0x00322C:
                value = this.fifo_dma_instance;
                break;
            case 0x003230:
                value = this.fifo_dma_state || 0x80000000;
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
                if((this.fifo_get & NV20_FIFO_CACHE_GET_MASK) !==
                    (this.fifo_cache1_put & NV20_FIFO_CACHE_GET_MASK))
                {
                    this.fifo_pull0 |= 0x100;
                }
                value = this.fifo_pull0;
                break;
            case 0x003254:
                value = this.fifo_pull1;
                break;
            case 0x003270:
                value = this.fifo_get;
                break;
            case 0x0032E0:
                value = this.fifo_engine;
                break;
            case 0x0032E4:
                value = this.fifo_dma_fetch;
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
            case 0x400724:
                value = this.graph_bpixel;
                break;
            case 0x400780:
                value = this.graph_channel_ctx_table;
                break;
            case 0x400820:
                value = this.graph_offset0;
                break;
            case 0x400850:
                value = this.graph_pitch0;
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
                this.crtc_raster_pos ^= 1;
                value = this.crtc_raster_pos;
                break;
            case 0x60080C:
                value = this.crtc_cursor_offset;
                break;
            case 0x600810:
                value = this.crtc_cursor_config;
                break;
            case 0x60081C:
                value = this.crtc_gpio_ext;
                break;
            case 0x600868:
                value = 0;
                break;
            case 0x6013B4:
            case 0x6013D4:
                value = this.prmcio_read32(offset);
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
                known = !!nv20_mmio_register_name(offset);
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
    value >>>= 0;
    var channel;

    if(offset >= NV20_PRAMIN_BASE && offset < NV20_PRAMIN_BASE + NV20_PRAMIN_SIZE)
    {
        this.ramin_write32(offset - NV20_PRAMIN_BASE, value);
        return true;
    }

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

    if(offset >= NV20_FIFO_USER_BASE && offset < NV20_FIFO_USER_BASE + NV20_FIFO_USER_SIZE)
    {
        return this.fifo_pio_user_write32(offset, value);
    }

    if(offset >= NV20_FIFO_DMA_USER_BASE && offset < NV20_FIFO_DMA_USER_BASE + NV20_FIFO_DMA_USER_SIZE)
    {
        return this.fifo_dma_user_write32(offset, value);
    }

    switch(offset)
    {
        case 0x000100:
            this.mc_soft_intr = !!(value >>> 31);
            return true;
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

        case 0x002080:
            this.fifo_cache_error &= ~value;
            return true;
        case 0x002100:
            this.fifo_intr &= ~value;
            return true;
        case 0x002140:
            this.fifo_intr_en = value;
            return true;
        case 0x002200:
            this.fifo_config = value;
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
        case 0x002410:
            this.fifo_runout_put = value;
            return true;
        case 0x002420:
            this.fifo_runout_get = value;
            return true;
        case 0x002500:
            this.fifo_caches = value;
            return true;
        case 0x002504:
            this.fifo_mode = value;
            return true;
        case 0x003200:
            this.fifo_cache1_push0 = value;
            return true;
        case 0x003204:
            this.fifo_cache1_push1 = value;
            this.fifo_set_active_channel(value & 0x7F);
            return true;
        case 0x003210:
            this.fifo_cache1_put = value;
            return true;
        case 0x003218:
            this.fifo_dma_dcount = value;
            channel = this.fifo_active_channel_state();
            this.fifo_load_channel_context(channel);
            channel.dma_dcount = value;
            return true;
        case 0x003220:
            this.fifo_dma_push = value;
            this.fifo_dma_kick(this.fifo_active_channel_state(), "cache1-dma-push");
            return true;
        case 0x00322C:
            this.fifo_dma_instance = value;
            channel = this.fifo_active_channel_state();
            channel.dma_instance = value & 0xFFFF;
            channel.context_loaded = true;
            return true;
        case 0x003230:
            this.fifo_dma_state = value;
            channel = this.fifo_active_channel_state();
            this.fifo_load_channel_context(channel);
            channel.dma_state = value;
            return true;
        case 0x003240:
            this.fifo_dma_put = value;
            channel = this.fifo_active_channel_state();
            this.fifo_load_channel_context(channel);
            channel.dma_put = value;
            this.fifo_dma_kick(channel, "cache1-dma-put");
            return true;
        case 0x003244:
            this.fifo_dma_get = value;
            channel = this.fifo_active_channel_state();
            this.fifo_load_channel_context(channel);
            channel.dma_get = value;
            return true;
        case 0x003248:
            this.fifo_ref_cnt = value;
            channel = this.fifo_active_channel_state();
            this.fifo_load_channel_context(channel);
            channel.ref = value;
            return true;
        case 0x003250:
            this.fifo_pull0 = value;
            return true;
        case 0x003254:
            this.fifo_pull1 = value;
            return true;
        case 0x003270:
            this.fifo_get = value & NV20_FIFO_CACHE_GET_MASK;
            if(this.fifo_get !== (this.fifo_cache1_put & NV20_FIFO_CACHE_GET_MASK))
            {
                this.fifo_intr |= NV20_FIFO_INTR_CACHE_ERROR;
            }
            else
            {
                this.fifo_intr &= ~NV20_FIFO_INTR_CACHE_ERROR;
                this.fifo_pull0 &= ~0x100;
            }
            return true;
        case 0x0032E0:
            this.fifo_engine = value;
            return true;
        case 0x0032E4:
            this.fifo_dma_fetch = value;
            channel = this.fifo_active_channel_state();
            this.fifo_load_channel_context(channel);
            channel.dma_fetch = value;
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
        case 0x400724:
            this.graph_bpixel = value;
            return true;
        case 0x400780:
            this.graph_channel_ctx_table = value;
            return true;
        case 0x400820:
            this.graph_offset0 = value;
            return true;
        case 0x400850:
            this.graph_pitch0 = value;
            return true;

        case 0x600100:
            this.crtc_intr &= ~value;
            return true;
        case 0x600140:
            this.crtc_intr_en = value;
            return true;
        case 0x600800:
            this.crtc_start = value;
            this.update_render_mode_from_crtc("pcrtc_start");
            return true;
        case 0x600804:
            this.crtc_config = value;
            this.update_render_mode_from_crtc("pcrtc_config");
            return true;
        case 0x60080C:
            this.crtc_cursor_offset = value;
            return true;
        case 0x600810:
            this.crtc_cursor_config = value;
            return true;
        case 0x60081C:
            this.crtc_gpio_ext = value;
            return true;
        case 0x6013B4:
        case 0x6013D4:
            this.prmcio_write32(offset, value);
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

    return !!nv20_mmio_register_name(offset);
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

    const name = nv20_mmio_register_name(offset);

    dbg_log(this.name + " mmio " + kind + " " + h(offset >>> 0, 6) +
            (name ? " (" + name + ")" : "") +
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
