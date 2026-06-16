// Minimal NVIDIA NV20/GeForce3 PCI shell.
//
// This intentionally stays at the PCI + BAR0/BAR1 + framebuffer bridge stage
// with enough PFIFO/RAMIN plumbing for drivers to create channels and submit
// commands. A small 2D PGRAPH subset is implemented for legacy framebuffer
// acceleration; full PGRAPH execution is a later milestone.

// For Types Only
import { CPU } from "../cpu.js";

import { CR0_PG, LOG_PCI, MMAP_BLOCK_SIZE } from "../const.js";
import { h } from "../lib.js";
import { dbg_log } from "../log.js";

const NV20_VENDOR_ID = 0x10DE;
const NV20_DEVICE_ID_GEFORCE3_TI_500 = 0x0202;
const NV20_SUBSYSTEM_VENDOR_ID = 0x107D;
const NV20_SUBSYSTEM_ID_GEFORCE3_TI_500 = 0x2863;

const NV20_DEFAULT_PCI_ID = 0x13 << 3;
const NV20_DEFAULT_MMIO_BASE = 0xF1000000;
const NV20_MMIO_SIZE = 16 * 1024 * 1024;
const NV20_DEFAULT_VRAM_BASE = 0xD0000000;
const NV20_DEFAULT_VRAM_SIZE = 64 * 1024 * 1024;
const NV20_DEFAULT_ROM_BASE = 0xFE000000;
const NV20_PRAMIN_BASE = 0x00700000;
const NV20_PRAMIN_SIZE = 1024 * 1024;
const NV20_PVIDEO_BASE = 0x00008000;
const NV20_PVIDEO_SIZE = 0x00001000;
const NV20_UNK010000_BASE = 0x00010000;
const NV20_UNK010000_SIZE = 0x00000100;
const NV20_PVIDEO_OVERLAY_BASE = 0x00200000;
const NV20_PVIDEO_OVERLAY_SIZE = 0x00001000;
const NV20_BAR2_SIZE = 0x00080000;
const NV20_MMIO_ROM_BASE = 0x00300000;
const NV20_MMIO_ROM_SIZE = 0x00010000;
const NV20_RAMIN_REVERSE_UNIT = 64;
const NV20_PMC_BOOT_0 = 0x020200A5;
const NV20_PFB_CFG0 = 0x00007FFF;

const NV20_PMC_INTR_PFIFO = 1 << 8;
const NV20_PMC_INTR_PGRAPH = 1 << 12;
const NV20_PMC_INTR_PTIMER = 1 << 20;
const NV20_PMC_INTR_PCRTC = 1 << 24;
const NV20_PMC_INTR_PBUS = 1 << 28;

const NV20_FIFO_CACHE_ENTRY_COUNT = 0x100;
const NV20_FIFO_CACHE_RING_ENTRY_COUNT = 0x40;
const NV20_FIFO_CACHE_GET_MASK = NV20_FIFO_CACHE_RING_ENTRY_COUNT * 4 - 1;
const NV20_FIFO_CACHE_EMPTY = 0x10;
const NV20_FIFO_INTR_CACHE_ERROR = 1 << 0;
const NV20_FIFO_INTR_DMA_PUSHER = 1 << 12;
const NV20_FIFO_INTR_DMA_PTE = 1 << 16;
const NV20_FIFO_DMA_PUSH_ACCESS = 1 << 0;
const NV20_FIFO_DMA_PUSH_STATE_BUSY = 1 << 4;
const NV20_FIFO_DMA_PUSH_BUFFER_EMPTY = 1 << 8;
const NV20_FIFO_DMA_PUSH_STATUS_SUSPENDED = 1 << 12;
const NV20_FIFO_DMA_PUSH_CONTROL_MASK =
    NV20_FIFO_DMA_PUSH_ACCESS |
    NV20_FIFO_DMA_PUSH_STATUS_SUSPENDED;
const NV20_GRAPH_DEBUG_RECENT_LIMIT = 96;
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
const NV20_FIFO_DMA_SYNC_KICK_LIMIT = 0x100;
const NV20_FIFO_DMA_ASYNC_KICK_LIMIT = 0x400;
const NV20_FIFO_METHOD_LOG_LIMIT = 32;
const NV20_RAMHT_ENTRY_SIZE = 8;
const NV20_RAMFC_NV10_STRIDE = 64;
const NV20_GRAPH_OBJECT_ENGINE = 1;
const NV20_VRAM_LOG_BLOCK_SIZE = 1024 * 1024;
const NV20_VRAM_LOG_SAMPLE_WRITES = 0x10000;
const NV20_MMIO_HOT_POLL_LOG_LIMIT = 64;
const NV20_DEFAULT_RENDER_WIDTH = 1024;
const NV20_DEFAULT_RENDER_HEIGHT = 768;
const NV20_DEFAULT_RENDER_BPP = 32;
const NV20_DEFAULT_RENDER_FORMAT = "xrgb8888";
const NV20_RENDER_DIRTY_LAYER_LIMIT = 32;
const NV20_DISPI_VERSION = 0xB0C5;
const NV20_DISPI_MAX_XRES = 2560;
const NV20_DISPI_MAX_YRES = 1600;
const NV20_DISPI_MAX_BPP = 32;
const NV20_2D_OPERATION_SRCCOPY = 3;
const NV20_ROP_SRCCOPY = 0xCC;

const NV20_CLASS_DMA_FROM_MEMORY = 0x0002;
const NV20_CLASS_DMA_TO_MEMORY = 0x0003;
const NV20_CLASS_DMA_IN_MEMORY = 0x003D;
const NV20_CLASS_CLIP = 0x0019;
const NV20_CLASS_RECT_NV1 = 0x001E;
const NV20_CLASS_RECT_NV4 = 0x005E;
const NV20_CLASS_BLIT_NV1 = 0x001F;
const NV20_CLASS_BLIT_NV4 = 0x005F;
const NV20_CLASS_BLIT_NV15 = 0x009F;
const NV20_CLASS_IFC_NV1 = 0x0021;
const NV20_CLASS_IFC_NV4 = 0x0061;
const NV20_CLASS_IIFC_NV4 = 0x0064;
const NV20_CLASS_IFC_NV5 = 0x0065;
const NV20_CLASS_IFC_NV10 = 0x008A;
const NV20_CLASS_IFC_NV30 = 0x038A;
const NV20_CLASS_IFC_NV40 = 0x308A;
const NV20_CLASS_SIFC_NV3 = 0x0036;
const NV20_CLASS_SIFC_NV4_LEGACY = 0x005C;
const NV20_CLASS_SIFC_NV4 = 0x0076;
const NV20_CLASS_SIFC_NV5 = 0x0066;
const NV20_CLASS_SIFC_NV30 = 0x0366;
const NV20_CLASS_SIFC_NV40 = 0x3066;
const NV20_CLASS_TEXUPLOAD_NV10 = 0x007B;
const NV20_CLASS_TEXUPLOAD_NV30 = 0x037B;
const NV20_CLASS_TEXUPLOAD_NV40 = 0x307B;
const NV20_CLASS_M2MF = 0x0039;
const NV20_CLASS_ROP = 0x0043;
const NV20_CLASS_PATTERN_NV1 = 0x0018;
const NV20_CLASS_PATTERN = 0x0044;
const NV20_CLASS_GDI_NV3 = 0x004B;
const NV20_CLASS_GDI_NV4 = 0x004A;
const NV20_CLASS_SWIZZLED_SURFACE_NV4 = 0x0052;
const NV20_CLASS_SWIZZLED_SURFACE_NV15 = 0x009E;
const NV20_CLASS_CHROMA = 0x0057;
const NV20_CLASS_BETA = 0x0072;
const NV20_CLASS_SIFM_NV10 = 0x0089;
const NV20_CLASS_SIFM_NV30 = 0x0389;
const NV20_CLASS_D3D_NV10 = 0x0096;
const NV20_CLASS_D3D_NV10_TCL = 0x0097;
const NV20_CLASS_D3D_NV15 = 0x0497;
const NV20_CLASS_D3D_NV20 = 0x0597;
const NV20_CLASS_SURFACE_2D_NV4 = 0x0042;
const NV20_CLASS_SURFACE_2D_NV10 = 0x0062;
const NV20_CLASS_SURFACE_2D_NV30 = 0x0362;
const NV20_CLASS_SURFACE_2D_NV40 = 0x3062;

const NV20_PRMCIO_CRTC_INDEX_COLOR = 0x6013D4;
const NV20_PRMCIO_CRTC_DATA_COLOR = 0x6013D5;
const NV20_PRMCIO_CRTC_INDEX_MONO = 0x6013B4;
const NV20_PRMCIO_CRTC_DATA_MONO = 0x6013B5;
const NV20_VGA_CRTC_MAX = 0x18;
const NV20_CRTC_DDC0_STATUS = 0x36;
const NV20_CRTC_DDC0_WRITE = 0x37;
const NV20_CRTC_I2C_READ = 0x3E;
const NV20_CRTC_I2C_WRITE = 0x3F;
const NV20_CRTC_DDC_LINES_HIGH = 0x0C;
const NV20_CRTC_DDC_WRITE_LINES_HIGH = 0x30;
const NV20_CRTC_RASTER_POLL_PHASES = 8;
const NV20_LEGACY_VGA_MEM_BASE = 0xA0000;
const NV20_LEGACY_VGA_MEM_SIZE = 0x20000;

const NV20_MIN_RENDER_WIDTH = 320;
const NV20_MIN_RENDER_HEIGHT = 200;
const NV20_MAX_RENDER_WIDTH = 4096;
const NV20_MAX_RENDER_HEIGHT = 4096;
const NV20_RENDER_COMMON_MODES = [
    [1920, 1200],
    [1600, 1200],
    [1440, 1080],
    [1280, 1024],
    [1152, 864],
    [1024, 768],
    [800, 600],
    [640, 480],
];

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
    [0x100240, "PFB_TILE_0"],
    [0x100244, "PFB_TLIMIT_0"],
    [0x100248, "PFB_TSIZE_0"],
    [0x100320, "PFB_CFG0"],
    [0x101000, "PEXTDEV_BOOT_0"],

    [0x400100, "PGRAPH_INTR"],
    [0x400108, "PGRAPH_NSOURCE"],
    [0x400140, "PGRAPH_INTR_EN"],
    [0x400144, "PGRAPH_CTX_CONTROL"],
    [0x400148, "PGRAPH_CTX_USER"],
    [0x40014C, "PGRAPH_CTX_SWITCH1"],
    [0x400150, "PGRAPH_CTX_SWITCH2"],
    [0x400154, "PGRAPH_CTX_SWITCH3"],
    [0x400158, "PGRAPH_CTX_SWITCH4"],
    [0x40032C, "PGRAPH_CTXCTL_CUR"],
    [0x400700, "PGRAPH_STATUS"],
    [0x400704, "PGRAPH_TRAPPED_ADDR"],
    [0x400708, "PGRAPH_TRAPPED_DATA"],
    [0x40070C, "PGRAPH_TRAPPED_DATA_HIGH"],
    [0x400718, "PGRAPH_NOTIFY"],
    [0x40071C, "PGRAPH_NOTIFY_INSTANCE"],
    [0x400720, "PGRAPH_FIFO"],
    [0x400724, "PGRAPH_BPIXEL"],
    [0x400780, "PGRAPH_CHANNEL_CTX_TABLE"],
    [0x400820, "PGRAPH_OFFSET0"],
    [0x400850, "PGRAPH_PITCH0"],
    [0x400900, "PGRAPH_TILE_0"],
    [0x400904, "PGRAPH_TLIMIT_0"],
    [0x400908, "PGRAPH_TSIZE_0"],

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

    const pvideo_name = nv20_pvideo_register_name(offset);

    if(pvideo_name)
    {
        return pvideo_name;
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

    if(offset >= NV20_MMIO_ROM_BASE && offset < NV20_MMIO_ROM_BASE + NV20_MMIO_ROM_SIZE)
    {
        return "PCI_ROM[" + h(offset - NV20_MMIO_ROM_BASE, 5) + "]";
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

    if(offset >= NV20_UNK010000_BASE && offset < NV20_UNK010000_BASE + NV20_UNK010000_SIZE)
    {
        return "UNK010000[" + h(offset - NV20_UNK010000_BASE, 4) + "]";
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

function nv20_pvideo_register_name(offset)
{
    offset >>>= 0;

    if(offset >= NV20_PVIDEO_BASE && offset < NV20_PVIDEO_BASE + NV20_PVIDEO_SIZE)
    {
        switch(offset)
        {
            case 0x008100: return "PVIDEO_INTR";
            case 0x008140: return "PVIDEO_INTR_EN";
            case 0x008700: return "PVIDEO_BUFFER";
            case 0x008704: return "PVIDEO_STOP";
            case 0x008910: return "PVIDEO_OFFSET[0]";
            case 0x008914: return "PVIDEO_OFFSET[1]";
            case 0x008918: return "PVIDEO_SIZE_IN[0]";
            case 0x00891C: return "PVIDEO_SIZE_IN[1]";
            case 0x008928: return "PVIDEO_LIMIT[0]";
            case 0x00892C: return "PVIDEO_LIMIT[1]";
            case 0x008930: return "PVIDEO_POINT_IN[0]";
            case 0x008934: return "PVIDEO_POINT_IN[1]";
            case 0x008938: return "PVIDEO_DS_DX[0]";
            case 0x00893C: return "PVIDEO_DS_DX[1]";
            case 0x008940: return "PVIDEO_DT_DY[0]";
            case 0x008944: return "PVIDEO_DT_DY[1]";
            default: return "PVIDEO[" + h(offset - NV20_PVIDEO_BASE, 4) + "]";
        }
    }

    if(offset >= NV20_PVIDEO_OVERLAY_BASE &&
        offset < NV20_PVIDEO_OVERLAY_BASE + NV20_PVIDEO_OVERLAY_SIZE)
    {
        switch(offset)
        {
            case 0x200140: return "PVIDEO_OVERLAY_INTR_EN";
            case 0x200200: return "PVIDEO_OVERLAY_CONTROL";
            case 0x200204: return "PVIDEO_OVERLAY_POINT_IN";
            case 0x200208: return "PVIDEO_OVERLAY_SIZE_IN";
            default: return "PVIDEO_OVERLAY[" + h(offset - NV20_PVIDEO_OVERLAY_BASE, 4) + "]";
        }
    }

    return "";
}

function nv20_mmio_hot_poll_read_offset(offset)
{
    offset >>>= 0;

    switch(offset)
    {
        case 0x000100: // PMC_INTR_0
        case 0x000140: // PMC_INTR_EN_0
        case 0x001100: // PBUS_INTR_0
        case 0x001140: // PBUS_INTR_EN_0
        case 0x002080: // PFIFO_CACHE_ERROR
        case 0x002100: // PFIFO_INTR_0
        case 0x002140: // PFIFO_INTR_EN_0
        case 0x002400: // PFIFO_RUNOUT_STATUS
        case 0x003214: // PFIFO_CACHE1_STATUS
        case 0x003220: // PFIFO_CACHE1_DMA_PUSH
        case 0x003230: // PFIFO_CACHE1_DMA_STATE
        case 0x003240: // PFIFO_CACHE1_DMA_PUT
        case 0x003244: // PFIFO_CACHE1_DMA_GET
        case 0x003250: // PFIFO_CACHE1_PULL0
        case 0x003270: // PFIFO_CACHE1_GET
        case 0x008100: // PVIDEO_INTR
        case 0x008140: // PVIDEO_INTR_EN
        case 0x008704: // PVIDEO_STOP
        case 0x009100: // PTIMER_INTR_0
        case 0x009140: // PTIMER_INTR_EN_0
        case 0x400100: // PGRAPH_INTR
        case 0x400108: // PGRAPH_NSOURCE
        case 0x400140: // PGRAPH_INTR_EN
        case 0x400144: // PGRAPH_CTX_CONTROL
        case 0x400148: // PGRAPH_CTX_USER
        case 0x400700: // PGRAPH_STATUS
        case 0x400718: // PGRAPH_NOTIFY
        case 0x40071C: // PGRAPH_NOTIFY_INSTANCE
        case 0x400720: // PGRAPH_FIFO
        case 0x600100: // PCRTC_INTR
        case 0x600140: // PCRTC_INTR_EN
        case 0x600808: // PCRTC_RASTER
        case 0x680300: // PRAMDAC_CURSOR_START
        case 0x200140: // PVIDEO_OVERLAY_INTR_EN
        case 0x200200: // PVIDEO_OVERLAY_CONTROL
            return true;
    }

    if((offset >= 0x601300 && offset < 0x601400) ||
        (offset >= 0x603300 && offset < 0x603400) ||
        (offset >= 0x681300 && offset < 0x681400) ||
        (offset >= 0x683300 && offset < 0x683400))
    {
        const port = offset & 0xFF;
        return port === 0xB4 || port === 0xB5 || port === 0xD4 ||
            port === 0xD5 || port === 0xDA;
    }

    return false;
}

function nv20_mmio_hot_poll_log_count(count)
{
    return count === 128 || count === 1024 ||
        count >= 8192 && (count & count - 1) === 0;
}

function nv20_mmio_vga_alias_port(offset)
{
    offset >>>= 0;

    if((offset >= 0x0C0300 && offset < 0x0C0400) ||
        (offset >= 0x0C2300 && offset < 0x0C2400) ||
        (offset >= 0x601300 && offset < 0x601400) ||
        (offset >= 0x603300 && offset < 0x603400) ||
        (offset >= 0x681300 && offset < 0x681400) ||
        (offset >= 0x683300 && offset < 0x683400))
    {
        return offset & 0xFFF;
    }

    return -1;
}

function nv20_mmio_vga_alias_head(offset)
{
    return offset >>> 13 & 1;
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

function nv20_render_format_from_bpp(bpp, fallback_format)
{
    switch(bpp)
    {
        case 8:
            return "indexed8";
        case 15:
            return "xrgb1555";
        case 16:
            return "rgb565";
        case 24:
            return "rgb888";
        case 32:
            return fallback_format === "rgba8888" || fallback_format === "xbgr8888" ?
                fallback_format : NV20_DEFAULT_RENDER_FORMAT;
        default:
            return fallback_format || NV20_DEFAULT_RENDER_FORMAT;
    }
}

function nv20_sane_render_bpp(bpp)
{
    return bpp === 8 || bpp === 15 || bpp === 16 || bpp === 24 || bpp === 32;
}

function nv20_i16(value)
{
    value &= 0xFFFF;
    return value & 0x8000 ? value - 0x10000 : value;
}

function nv20_unpack_xy(value)
{
    return {
        x: nv20_i16(value),
        y: nv20_i16(value >>> 16),
    };
}

function nv20_unpack_wh(value)
{
    return {
        w: value & 0xFFFF,
        h: value >>> 16,
    };
}

function nv20_unpack_yx(value)
{
    return {
        x: nv20_i16(value >>> 16),
        y: nv20_i16(value),
    };
}

function nv20_unpack_hw(value)
{
    return {
        w: value >>> 16,
        h: value & 0xFFFF,
    };
}

function nv20_unpack_xy12d4(value)
{
    return {
        x: nv20_i16(value) >> 4,
        y: nv20_i16(value >>> 16) >> 4,
    };
}

function nv20_surface_format_from_bpp(bpp)
{
    switch(bpp)
    {
        case 8:
            return 0x1;
        case 15:
            return 0x3;
        case 16:
            return 0x4;
        case 32:
            return 0x7;
        default:
            return 0;
    }
}

function nv20_surface_bpp_from_format(format, fallback_bpp)
{
    switch(format & 0xFF)
    {
        case 0x1:
            return 8;
        case 0x2:
        case 0x3:
        case 0x4:
        case 0x5:
            return 16;
        case 0x6:
        case 0x7:
        case 0x8:
        case 0x9:
        case 0xA:
        case 0xB:
            return 32;
        default:
            return fallback_bpp || NV20_DEFAULT_RENDER_BPP;
    }
}

function nv20_swizzled_surface_bpp_from_format(format, fallback_bpp)
{
    switch(format & 0xFFFF)
    {
        case 0x1:
            return 8;
        case 0x2:
        case 0x4:
            return 16;
        case 0x6:
        case 0xA:
        case 0xB:
            return 32;
        default:
            return fallback_bpp || NV20_DEFAULT_RENDER_BPP;
    }
}

function nv20_swizzle_index(x, y, width, height)
{
    x >>>= 0;
    y >>>= 0;
    width >>>= 0;
    height >>>= 0;

    var index = 0;
    var out_bit = 1;

    for(var bit = 1; bit < width || bit < height; bit <<= 1)
    {
        if(bit < width)
        {
            if(x & bit)
            {
                index |= out_bit;
            }

            out_bit <<= 1;
        }

        if(bit < height)
        {
            if(y & bit)
            {
                index |= out_bit;
            }

            out_bit <<= 1;
        }
    }

    return index >>> 0;
}

function nv20_source_color_bpp_from_format(format, fallback_bpp, surface_format, allow_mono)
{
    if((surface_format & 0xFF) === 0x1)
    {
        return 8;
    }

    switch(format & 0xFF)
    {
        case 0x0:
            return allow_mono ? 1 : fallback_bpp || NV20_DEFAULT_RENDER_BPP;
        case 0x1:
        case 0x2:
        case 0x3:
            return 16;
        case 0x4:
        case 0x5:
            return 32;
        default:
            return fallback_bpp || NV20_DEFAULT_RENDER_BPP;
    }
}

function nv20_ifc_bpp_from_format(format, fallback_bpp, surface_format, allow_mono)
{
    return nv20_source_color_bpp_from_format(format, fallback_bpp, surface_format, allow_mono);
}

function nv20_iifc_bpp_from_format(format)
{
    return nv20_ifc_bpp_from_format(format || 1, 16, 0, false);
}

function nv20_solid_bpp_from_format(format, fallback_bpp, state)
{
    const format8 = format & 0xFF;

    if(state && nv20_is_gdi_class(state.class_id) && fallback_bpp === 32 && format8 === 3)
    {
        return 32;
    }

    return nv20_source_color_bpp_from_format(format8, fallback_bpp, 0, false);
}

function nv20_d3d_color_format(class_id, format)
{
    class_id &= 0xFFFF;
    return class_id <= NV20_CLASS_D3D_NV10_TCL ? format & 0xF : format & 0x1F;
}

function nv20_d3d_bpp_from_format(class_id, format, fallback_bpp)
{
    switch(nv20_d3d_color_format(class_id, format))
    {
        case 0x9:
            return 8;
        case 0x3:
            return 16;
        case 0x4:
        case 0x5:
        case 0x8:
            return 32;
        default:
            return fallback_bpp || NV20_DEFAULT_RENDER_BPP;
    }
}

function nv20_d3d_depth_bytes_from_format(class_id, format, fallback_bytes)
{
    class_id &= 0xFFFF;
    const depth_format = class_id <= NV20_CLASS_D3D_NV10_TCL ?
        format >>> 4 & 0xF :
        format >>> 5 & 0x7;

    switch(depth_format)
    {
        case 0:
            return nv20_render_bytes_per_pixel(nv20_d3d_bpp_from_format(class_id, format, 32));
        case 1:
            return 2;
        case 2:
            return 4;
        default:
            return fallback_bytes || 4;
    }
}

function nv20_surface_format_from_d3d_format(class_id, format)
{
    switch(nv20_d3d_color_format(class_id, format))
    {
        case 0x9:
            return 0x1;
        case 0x3:
            return 0x4;
        case 0x8:
            return 0xA;
        case 0x4:
        case 0x5:
            return 0x7;
        default:
            return 0;
    }
}

function nv20_sifm_bpp_from_format(format, fallback_bpp)
{
    switch(format & 0xFF)
    {
        case 0x8:
            return 8;
        case 0x1:
        case 0x2:
        case 0x7:
            return 16;
        case 0x3:
        case 0x4:
            return 32;
        default:
            return fallback_bpp || NV20_DEFAULT_RENDER_BPP;
    }
}

function nv20_color_to_bytes(color, src_bpp, src_format, dst_bpp, dst_format)
{
    color >>>= 0;
    src_bpp = src_bpp || dst_bpp;
    dst_bpp = dst_bpp || src_bpp;

    var r = 0;
    var g = 0;
    var b = 0;

    if(src_bpp === 8)
    {
        r = g = b = color & 0xFF;
    }
    else if(src_bpp === 15 ||
        (src_bpp === 16 && (src_format === 2 || src_format === 3)))
    {
        r = (color >> 10 & 0x1F) * 0xFF / 0x1F | 0;
        g = (color >> 5 & 0x1F) * 0xFF / 0x1F | 0;
        b = (color & 0x1F) * 0xFF / 0x1F | 0;
    }
    else if(src_bpp === 16)
    {
        r = (color >> 11 & 0x1F) * 0xFF / 0x1F | 0;
        g = (color >> 5 & 0x3F) * 0xFF / 0x3F | 0;
        b = (color & 0x1F) * 0xFF / 0x1F | 0;
    }
    else
    {
        r = color >> 16 & 0xFF;
        g = color >> 8 & 0xFF;
        b = color & 0xFF;
    }

    if(dst_bpp === 8)
    {
        return [(r * 30 + g * 59 + b * 11) / 100 & 0xFF];
    }

    if(dst_bpp === 15 || (dst_bpp === 16 && ((dst_format & 0xFF) === 2 || (dst_format & 0xFF) === 3)))
    {
        const pixel = (r * 0x1F / 0xFF | 0) << 10 |
            (g * 0x1F / 0xFF | 0) << 5 |
            (b * 0x1F / 0xFF | 0);
        return [pixel & 0xFF, pixel >>> 8];
    }

    if(dst_bpp === 16)
    {
        const pixel = (r * 0x1F / 0xFF | 0) << 11 |
            (g * 0x3F / 0xFF | 0) << 5 |
            (b * 0x1F / 0xFF | 0);
        return [pixel & 0xFF, pixel >>> 8];
    }

    return [b, g, r, color >>> 24];
}

function nv20_clamp8(value)
{
    value = value + 0.5 | 0;

    if(value < 0)
    {
        return 0;
    }

    if(value > 0xFF)
    {
        return 0xFF;
    }

    return value;
}

function nv20_sign_extend(value, bits)
{
    const shift = 32 - bits;
    return value << shift >> shift;
}

function nv20_pixel_channels(color, bpp, format)
{
    color >>>= 0;
    bpp = bpp || 32;
    format &= 0xFF;

    if(bpp === 8)
    {
        const gray = color & 0xFF;
        return {
            r: gray,
            g: gray,
            b: gray,
            a: 0xFF,
        };
    }

    if(bpp === 15 || (bpp === 16 && (format === 2 || format === 3)))
    {
        return {
            r: (color >> 10 & 0x1F) * 0xFF / 0x1F | 0,
            g: (color >> 5 & 0x1F) * 0xFF / 0x1F | 0,
            b: (color & 0x1F) * 0xFF / 0x1F | 0,
            a: 0xFF,
        };
    }

    if(bpp === 16)
    {
        return {
            r: (color >> 11 & 0x1F) * 0xFF / 0x1F | 0,
            g: (color >> 5 & 0x3F) * 0xFF / 0x3F | 0,
            b: (color & 0x1F) * 0xFF / 0x1F | 0,
            a: 0xFF,
        };
    }

    return {
        r: color >> 16 & 0xFF,
        g: color >> 8 & 0xFF,
        b: color & 0xFF,
        a: color >>> 24,
    };
}

function nv20_rop_byte(rop, src, dst, pat)
{
    rop &= 0xFF;
    src &= 0xFF;
    dst &= 0xFF;
    pat &= 0xFF;

    if(rop === NV20_ROP_SRCCOPY)
    {
        return src;
    }

    var value = 0;

    for(var bit = 0; bit < 8; bit++)
    {
        const index = ((pat >>> bit & 1) << 2) |
            ((src >>> bit & 1) << 1) |
            (dst >>> bit & 1);

        if(rop >>> index & 1)
        {
            value |= 1 << bit;
        }
    }

    return value;
}

function nv20_object_class_from_word(word)
{
    const class_id = word & 0xFFFF;

    return class_id & 0xFF00 ? class_id & 0xFF : class_id;
}

function nv20_class_name(class_id)
{
    switch(class_id & 0xFFFF)
    {
        case NV20_CLASS_SURFACE_2D_NV4:
            return "NV4_SURFACE_2D";
        case NV20_CLASS_SURFACE_2D_NV10:
            return "NV10_SURFACE_2D";
        case NV20_CLASS_SURFACE_2D_NV30:
            return "NV30_SURFACE_2D";
        case NV20_CLASS_SURFACE_2D_NV40:
            return "NV40_SURFACE_2D";
        case NV20_CLASS_RECT_NV1:
            return "NV1_RECT";
        case NV20_CLASS_RECT_NV4:
            return "NV4_RECT";
        case NV20_CLASS_BLIT_NV1:
            return "NV1_BLIT";
        case NV20_CLASS_BLIT_NV4:
            return "NV4_BLIT";
        case NV20_CLASS_BLIT_NV15:
            return "NV15_BLIT";
        case NV20_CLASS_IFC_NV1:
            return "NV1_IFC";
        case NV20_CLASS_IFC_NV4:
            return "NV4_IFC";
        case NV20_CLASS_IIFC_NV4:
            return "NV4_IIFC";
        case NV20_CLASS_IFC_NV5:
            return "NV5_IFC";
        case NV20_CLASS_IFC_NV10:
            return "NV10_IFC";
        case NV20_CLASS_SIFC_NV3:
            return "NV3_SIFC";
        case NV20_CLASS_SIFC_NV4_LEGACY:
            return "NV4_LEGACY_SIFC";
        case NV20_CLASS_SIFC_NV4:
            return "NV4_SIFC";
        case NV20_CLASS_SIFC_NV5:
            return "NV5_SIFC";
        case NV20_CLASS_TEXUPLOAD_NV10:
            return "NV10_TEXUPLOAD";
        case NV20_CLASS_M2MF:
            return "NV3_M2MF";
        case NV20_CLASS_ROP:
            return "NV4_ROP";
        case NV20_CLASS_PATTERN_NV1:
            return "NV1_PATTERN";
        case NV20_CLASS_PATTERN:
            return "NV4_PATTERN";
        case NV20_CLASS_GDI_NV3:
            return "NV3_GDI";
        case NV20_CLASS_GDI_NV4:
            return "NV4_GDI";
        case NV20_CLASS_SWIZZLED_SURFACE_NV4:
            return "NV4_SWIZZLED_SURFACE";
        case NV20_CLASS_SWIZZLED_SURFACE_NV15:
            return "NV15_SWIZZLED_SURFACE";
        case NV20_CLASS_CHROMA:
            return "NV4_CHROMA";
        case NV20_CLASS_BETA:
            return "NV4_BETA";
        case NV20_CLASS_SIFM_NV10:
            return "NV10_SIFM";
        case NV20_CLASS_SIFM_NV30:
            return "NV30_SIFM";
        case NV20_CLASS_D3D_NV10:
            return "NV10_D3D";
        case NV20_CLASS_D3D_NV10_TCL:
            return "NV10_D3D_TCL";
        case NV20_CLASS_D3D_NV15:
            return "NV15_D3D";
        case NV20_CLASS_D3D_NV20:
            return "NV20_D3D";
        case NV20_CLASS_CLIP:
            return "NV1_CLIP";
        default:
            return h(class_id & 0xFFFF, 4);
    }
}

function nv20_is_dma_class(class_id)
{
    class_id &= 0xFFFF;
    return class_id === NV20_CLASS_DMA_FROM_MEMORY ||
        class_id === NV20_CLASS_DMA_TO_MEMORY ||
        class_id === NV20_CLASS_DMA_IN_MEMORY;
}

function nv20_is_surface2d_class(class_id)
{
    class_id &= 0xFFFF;
    return class_id === NV20_CLASS_SURFACE_2D_NV4 ||
        class_id === NV20_CLASS_SURFACE_2D_NV10 ||
        class_id === NV20_CLASS_SURFACE_2D_NV30 ||
        class_id === NV20_CLASS_SURFACE_2D_NV40;
}

function nv20_is_rect_class(class_id)
{
    class_id &= 0xFFFF;
    return class_id === NV20_CLASS_RECT_NV1 ||
        class_id === NV20_CLASS_RECT_NV4;
}

function nv20_is_blit_class(class_id)
{
    class_id &= 0xFFFF;
    return class_id === NV20_CLASS_BLIT_NV1 ||
        class_id === NV20_CLASS_BLIT_NV4 ||
        class_id === NV20_CLASS_BLIT_NV15;
}

function nv20_is_ifc_class(class_id)
{
    class_id &= 0xFFFF;
    return class_id === NV20_CLASS_IFC_NV1 ||
        class_id === NV20_CLASS_IFC_NV4 ||
        class_id === NV20_CLASS_IFC_NV5 ||
        class_id === NV20_CLASS_IFC_NV10 ||
        class_id === NV20_CLASS_IFC_NV30 ||
        class_id === NV20_CLASS_IFC_NV40;
}

function nv20_is_iifc_class(class_id)
{
    return (class_id & 0xFFFF) === NV20_CLASS_IIFC_NV4;
}

function nv20_is_sifc_class(class_id)
{
    class_id &= 0xFFFF;
    return class_id === NV20_CLASS_SIFC_NV3 ||
        class_id === NV20_CLASS_SIFC_NV4_LEGACY ||
        class_id === NV20_CLASS_SIFC_NV4 ||
        class_id === NV20_CLASS_SIFC_NV5 ||
        class_id === NV20_CLASS_SIFC_NV30 ||
        class_id === NV20_CLASS_SIFC_NV40;
}

function nv20_is_texupload_class(class_id)
{
    class_id &= 0xFFFF;
    return class_id === NV20_CLASS_TEXUPLOAD_NV10 ||
        class_id === NV20_CLASS_TEXUPLOAD_NV30 ||
        class_id === NV20_CLASS_TEXUPLOAD_NV40;
}

function nv20_is_m2mf_class(class_id)
{
    return (class_id & 0xFFFF) === NV20_CLASS_M2MF;
}

function nv20_is_gdi_class(class_id)
{
    class_id &= 0xFFFF;
    return class_id === NV20_CLASS_GDI_NV3 ||
        class_id === NV20_CLASS_GDI_NV4;
}

function nv20_is_pattern_class(class_id)
{
    class_id &= 0xFFFF;
    return class_id === NV20_CLASS_PATTERN_NV1 ||
        class_id === NV20_CLASS_PATTERN;
}

function nv20_is_swizzled_surface_class(class_id)
{
    class_id &= 0xFFFF;
    return class_id === NV20_CLASS_SWIZZLED_SURFACE_NV4 ||
        class_id === NV20_CLASS_SWIZZLED_SURFACE_NV15;
}

function nv20_is_sifm_class(class_id)
{
    class_id &= 0xFFFF;
    return class_id === NV20_CLASS_SIFM_NV10 ||
        class_id === NV20_CLASS_SIFM_NV30;
}

function nv20_is_d3d_class(class_id)
{
    class_id &= 0xFFFF;
    return class_id === NV20_CLASS_D3D_NV10 ||
        class_id === NV20_CLASS_D3D_NV10_TCL ||
        class_id === NV20_CLASS_D3D_NV15 ||
        class_id === NV20_CLASS_D3D_NV20;
}

function nv20_is_d3d_known_method(method)
{
    method &= 0x1FFC;
    const word = method >>> 2;

    return word === 0x000 ||
        word >= 0x048 && word <= 0x04C ||
        word >= 0x061 && word <= 0x06A ||
        word >= 0x080 && word <= 0x7FF;
}

function nv20_is_d3d_like_method(method)
{
    method &= 0x1FFC;

    return nv20_is_d3d_known_method(method);
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
    regs[NV20_CRTC_DDC0_STATUS] = NV20_CRTC_DDC_LINES_HIGH;
    regs[NV20_CRTC_DDC0_WRITE] = NV20_CRTC_DDC_WRITE_LINES_HIGH;
    regs[NV20_CRTC_I2C_READ] = NV20_CRTC_DDC_LINES_HIGH;
    regs[NV20_CRTC_I2C_WRITE] = NV20_CRTC_DDC_WRITE_LINES_HIGH;
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

function nv20_graph_debug_2d_default()
{
    return {
        "methods": 0,
        "data_methods": 0,
        "fill": 0,
        "blit": 0,
        "upload_words": 0,
        "upload": 0,
        "mono_words": 0,
        "iifc": 0,
        "blend": 0,
        "blend_copy": 0,
        "blend_skip": 0,
        "chroma_skip": 0,
        "m2mf": 0,
        "sifm": 0,
        "gdi_rect": 0,
        "gdi_image": 0,
        "d3d_depth_clear_skip": 0,
        "accel": 0,
        "last": null,
        "by_class": {},
        "by_method": {},
        "recent": [],
    };
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
    this["debug_build_tag"] = "nv20-ext-lfb-no-vram-clear-20260611";
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
    this.vram_fast_memory = options["fast_lfb"] !== false &&
        cpu.devices && cpu.devices.vga && cpu.devices.vga.svga_memory || null;
    this.vram_fast_size = this.vram_fast_memory ?
        Math.min(vram_size, this.vram_fast_memory.length) : 0;
    this.vram_fast_dirty = false;
    this.vram_fast_dirty_min = this.vram_fast_size;
    this.vram_fast_dirty_max = 0;
    this.vram_fast_discard_svga_dirty = false;
    this.vram_fast_sync_key = "";
    this.option_rom = option_rom;

    dbg_log(this.name + " build tag " + this["debug_build_tag"], LOG_PCI);

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
    this.vram_trace = options.vram_trace === true;
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
    this.render_format = options.render_format ||
        nv20_render_format_from_bpp(this.render_bpp, NV20_DEFAULT_RENDER_FORMAT);
    this.render_frame_size = Math.min(this.vram_size - this.render_offset, this.render_stride * this.render_height);
    this.render_dirty_min = this.render_offset + this.render_frame_size;
    this.render_dirty_max = this.render_offset;
    this.render_dirty_rows = null;
    this.render_dirty_row_min = this.render_height;
    this.render_dirty_row_max = 0;
    this.render_active = false;
    this.render_initialized = false;
    this.render_pending = false;
    this.render_update_count = 0;
    this.render_buffer = null;
    this.render_image_data = null;
    this.render_source = "default";
    this.render_surface_inferred = false;
    this.render_surface_pitch = 0;
    this.render_surface_offset = 0;
    this.render_surface_bpp = 0;
    this["debug_render_mode"] = {
        "width": this.render_width,
        "height": this.render_height,
        "bpp": this.render_bpp,
        "stride": this.render_stride,
        "offset": this.render_offset,
        "source": this.render_source,
        "surface": false,
        "tile": false,
        "tile_pitch": 0,
    };
    this.screen = options.screen || cpu.devices.vga && cpu.devices.vga.screen;
    this.bus = options.bus || cpu.devices.vga && cpu.devices.vga.bus;
    this.mmio_registers = new Map();
    this.mmio_trace = options.mmio_trace === true || options.mmio_trace_all === true;
    this.mmio_trace_all = !!options.mmio_trace_all;
    this.mmio_seen_reads = new Set();
    this.mmio_seen_writes = new Set();
    this.mmio_hot_poll_counts = new Map();
    this.mmio_hot_poll_log_count = 0;
    this.mmio_hot_poll_log_limit = options.mmio_hot_poll_log_limit || NV20_MMIO_HOT_POLL_LOG_LIMIT;
    this.mmio_hot_poll_suppressed = false;
    this["debug_mmio_hot_poll"] = null;
    this.fifo_trace = options.fifo_trace === true;
    this.fifo_log_method_limit = options.fifo_log_method_limit || NV20_FIFO_METHOD_LOG_LIMIT;
    this.fifo_method_log_count = 0;
    this.missing_trace = options.missing_trace !== false;
    this.missing_log_limit = options.missing_log_limit || 256;
    this.missing_log_count = 0;
    this.missing_log_suppressed = false;
    this.missing_commands = new Map();
    this["debug_missing_commands"] = {
        total: 0,
        unique: 0,
        last: null,
    };

    this.pci = cpu.devices.pci;
    this.hide_default_vga_pci = options.hide_default_vga_pci !== false;
    this.default_vga_pci_hidden_logged = false;
    this.default_vga_display_taken_over = false;
    this.pci_config_space = null;
    this.pci_config_space8 = null;

    this.mc_soft_intr = false;
    this.mc_intr_en = 0;
    this.mc_enable = 0;
    this.irq_level = false;

    this.bus_intr = 0;
    this.bus_intr_en = 0;

    this.fifo_cache_error = 0;
    this.fifo_intr = 0;
    this.fifo_intr_en = 0;
    this.fifo_wait_notify = false;
    this.fifo_wait_flip = false;
    this.fifo_wait_soft = false;
    this.fifo_wait_acquire = false;
    this.fifo_wait_log_count = 0;
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
    this.fifo_last_dma_instance = 0;
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

    this.video_intr = 0;
    this.video_intr_en = 0;
    this.video_stop = 0;
    this.pvideo_regs = new Map();

    this.straps0_primary_original = options.straps0_primary || 0;
    this.straps0_primary = this.straps0_primary_original;
    this.fb_boot_0 = NV20_PMC_BOOT_0;
    this.fb_cfg = NV20_PFB_CFG0;
    this.fb_cfg0 = NV20_PFB_CFG0;

    this.graph_intr = 0;
    this.graph_intr_en = 0;
    this.graph_nsource = 0;
    this.graph_ctx_control = 0;
    this.graph_ctx_user = 0;
    this.graph_ctx_switch1 = 0;
    this.graph_ctx_switch2 = 0;
    this.graph_ctx_switch3 = 0;
    this.graph_ctx_switch4 = 0;
    this.graph_ctxctl_cur = 0;
    this.graph_status = 0;
    this.graph_trapped_addr = 0;
    this.graph_trapped_data = 0;
    this.graph_trapped_data_high = 0;
    this.graph_flip_read = 0;
    this.graph_flip_write = 0;
    this.graph_flip_modulo = 0;
    this.graph_notify = 0;
    this.graph_notify_instance = 0;
    this.graph_fifo = 0;
    this.graph_bpixel = 0;
    this.graph_channel_ctx_table = 0;
    this.graph_offset0 = 0;
    this.graph_pitch0 = 0;
    this.fb_tile0_flags = 0;
    this.fb_tile0_limit = 0;
    this.fb_tile0_pitch = 0;
    this.graph_object_states = new Map();
    this["graph_accel_count"] = 0;
    this["graph_d3d_method_count"] = 0;
    this["graph_unhandled_method_count"] = 0;
    this["graph_last_unhandled_method"] = null;
    this["graph_unhandled_log_count"] = 0;
    this["graph_mono_upload_count"] = 0;
    this["graph_debug_2d"] = nv20_graph_debug_2d_default();
    this.missing_log_count = 0;
    this.missing_log_suppressed = false;
    this.missing_commands.clear();
    this["debug_missing_commands"] = {
        total: 0,
        unique: 0,
        last: null,
    };

    this.crtc_intr = 0;
    this.crtc_intr_en = 0;
    this.crtc_start = 0;
    this.crtc_config = 0;
    this.crtc_raster_pos = 0;
    this.crtc_raster_counter = 0;
    this.crtc_cursor_offset = 0;
    this.crtc_cursor_config = 0;
    this.crtc_gpio_ext = 0;
    this.crtc_engine_ctrl = 0;
    this.crtc_status_read_count = 0;
    this.prmcio_crtc_index = 0;
    this.prmcio_crtc_regs = new Uint8Array(0x100);
    nv20_init_default_crtc_regs(this.prmcio_crtc_regs);
    this.rma_addr = 0;
    this.legacy_vga_memory = new Uint8Array(NV20_LEGACY_VGA_MEM_SIZE);
    this.legacy_vga_attr_regs = new Uint8Array(0x20);
    this.legacy_vga_seq_regs = new Uint8Array(0x100);
    this.legacy_vga_graphics_regs = new Uint8Array(0x100);
    this.legacy_vga_dac_data = new Uint8Array(0x300);
    this.reset_legacy_vga_shadow();
    this.reset_dispi_shadow();

    this.ramdac_cursor_start = 0;
    this.hw_cursor_enabled = false;
    this.hw_cursor_vram = true;
    this.hw_cursor_bpp32 = false;
    this.hw_cursor_size = 32;
    this.hw_cursor_offset = 0;
    this.hw_cursor_x = 0;
    this.hw_cursor_y = 0;
    this["debug_hw_cursor"] = null;
    this.ramdac_vpll = 0;
    this.ramdac_pll_select = 0;
    this.ramdac_vpll_b = 0;
    this.ramdac_general_control = 0;
    this.ramdac_fp_tg_control = 0;
    this.ramdac_dacclk = 0;

    this.pci_space = [
        // 00: vendor/device
        NV20_VENDOR_ID & 0xFF, NV20_VENDOR_ID >> 8,
        NV20_DEVICE_ID_GEFORCE3_TI_500 & 0xFF, NV20_DEVICE_ID_GEFORCE3_TI_500 >> 8,

        // 04: command/status. The status bits mirror Bochs' GeForce3 PCI
        // config: capability list, fast back-to-back, and medium DEVSEL.
        0x06, 0x00, 0xB0, 0x02,

        // 08: revision, prog-if, subclass, class (VGA compatible controller)
        0xA3, 0x00, 0x00, 0x03,
        // 0C: cache line, latency, header type, BIST
        0x00, 0x00, 0x00, 0x00,

        // 10: BAR0, MMIO registers
        mmio_base & 0xFF, mmio_base >> 8 & 0xFF, mmio_base >> 16 & 0xFF, mmio_base >>> 24,
        // 14: BAR1, framebuffer aperture. Bit 3 marks prefetchable memory.
        vram_base & 0xFF | 0x08, vram_base >> 8 & 0xFF, vram_base >> 16 & 0xFF, vram_base >>> 24,
        // 18: BAR2, small prefetchable RAMIN/PRAMIN aperture
        0x08, 0x00, 0x00, 0x00,
        // 1C..27: unused BARs
        0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00,

        // 28: CardBus CIS pointer
        0x00, 0x00, 0x00, 0x00,
        // 2C: subsystem vendor/device
        NV20_SUBSYSTEM_VENDOR_ID & 0xFF, NV20_SUBSYSTEM_VENDOR_ID >> 8,
        NV20_SUBSYSTEM_ID_GEFORCE3_TI_500 & 0xFF, NV20_SUBSYSTEM_ID_GEFORCE3_TI_500 >> 8,
        // 30: expansion ROM base, disabled by bit 0 until the guest enables it
        this.pci_rom_address & 0xFF, this.pci_rom_address >> 8 & 0xFF,
        this.pci_rom_address >> 16 & 0xFF, this.pci_rom_address >>> 24,
        // 34: capabilities pointer
        0x60, 0x00, 0x00, 0x00,
        // 38: reserved
        0x00, 0x00, 0x00, 0x00,
        // 3C: interrupt line, interrupt pin, min grant, max latency
        0x00, 0x01, 0x00, 0x00,
        // 40: subsystem mirror used by NVIDIA drivers
        NV20_SUBSYSTEM_VENDOR_ID & 0xFF, NV20_SUBSYSTEM_VENDOR_ID >> 8,
        NV20_SUBSYSTEM_ID_GEFORCE3_TI_500 & 0xFF, NV20_SUBSYSTEM_ID_GEFORCE3_TI_500 >> 8,
        // 44: AGP capability, version 2.0, terminal capability
        0x02, 0x00, 0x20, 0x00,
        // 48: AGP status, 1x/2x/4x supported, request queue depth 0x1F
        0x07, 0x00, 0x00, 0x1F,
        // 4C: AGP command, written by the guest
        0x00, 0x00, 0x00, 0x00,
        // 50: ROM shadow control, 54: NVIDIA private config byte
        0x00, 0x00, 0x00, 0x00,
        0x01, 0x00, 0x00, 0x00,
        // 58..5F: reserved
        0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00,
        // 60: power-management capability, next points to AGP at 0x44
        0x01, 0x44, 0x02, 0x00,
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
            on_remap: function(from, to) {
                this.vram_base = to >>> 0;
                this.update_fast_lfb_mapping();
            },
        },
        {
            size: NV20_BAR2_SIZE,
            remappable: true,
            read8: this.bar2_read8,
            write8: this.bar2_write8,
            read32: this.bar2_read32,
            write32: this.bar2_write32,
            on_remap: function(from, to) { this.bar2_base = to; },
        },
    ];

    this.pci_config_space = this.pci.register_device(this);
    this.pci_config_space8 = new Uint8Array(this.pci_config_space.buffer);
    this.restore_pci_readonly_config();
    this.update_fast_lfb_mapping();
    this.hide_default_vga_pci_device();

    this.register_rma_io();
    this.register_legacy_vga_io();
    this.register_legacy_vga_memory();
}

NV20GeForce.prototype.hide_default_vga_pci_device = function()
{
    if(!this.hide_default_vga_pci)
    {
        return;
    }

    const vga = this.cpu && this.cpu.devices && this.cpu.devices.vga;

    if(!vga || vga === this || vga.pci_hidden_by_geforce === this)
    {
        return;
    }

    const previous_hidden = vga.pci_hidden;
    const geforce = this;

    vga.pci_hidden_by_geforce = this;
    vga.pci_hidden = function()
    {
        const inherited = typeof previous_hidden === "function" ? previous_hidden.call(vga) : !!previous_hidden;
        return inherited || geforce.should_hide_default_vga_pci_device();
    };

    dbg_log("geforce-nv20 will hide default vga pci function after guest paging bdf=" +
            h(vga.pci_id || 0, 4), LOG_PCI);
};

NV20GeForce.prototype.should_hide_default_vga_pci_device = function()
{
    const cpu = this.cpu;
    const hide = !!(this.hide_default_vga_pci &&
        cpu && cpu.protected_mode && cpu.protected_mode[0] &&
        cpu.cr && (cpu.cr[0] & CR0_PG));

    if(hide && !this.default_vga_pci_hidden_logged)
    {
        this.default_vga_pci_hidden_logged = true;
        dbg_log("geforce-nv20 hiding default vga pci function in paged protected mode", LOG_PCI);
        this.take_over_default_vga_display();
    }

    return hide;
};

NV20GeForce.prototype.take_over_default_vga_display = function()
{
    if(this.default_vga_display_taken_over)
    {
        return;
    }

    this.default_vga_display_taken_over = true;
    this.dispi_ignore_boot_mode_sets = 1;

    const mode = this.crtc_render_mode();

    if(mode)
    {
        this.render_surface_inferred = false;
        this.set_render_mode(mode.width, mode.height, mode.bpp, mode.stride, mode.offset, "crtc-takeover");
    }

    this.dispi_turn_off_vga_svga();
    this.activate_rendering();
    this.render_mark_dirty_rect(0, 0, this.render_width, this.render_height);
    this.schedule_render();
};

NV20GeForce.prototype.reset = function()
{
    dbg_log(this.name + " reset", LOG_PCI);

    this.mmio_registers.clear();
    this.mmio_hot_poll_counts.clear();
    this.mmio_hot_poll_log_count = 0;
    this.mmio_hot_poll_suppressed = false;
    this["debug_mmio_hot_poll"] = null;

    this.vram_logged_write_blocks.clear();
    this.vram_write_count = 0;
    this.vram_dirty_min = this.vram_size;
    this.vram_dirty_max = 0;
    this.vram_fast_dirty = false;
    this.vram_fast_dirty_min = this.vram_fast_size;
    this.vram_fast_dirty_max = 0;
    this.vram_fast_discard_svga_dirty = false;

    this.render_active = false;
    this.render_initialized = false;
    this.render_pending = false;
    this.default_vga_pci_hidden_logged = false;
    this.default_vga_display_taken_over = false;
    this.render_update_count = 0;
    this.render_buffer = null;
    this.render_image_data = null;
    this.render_surface_inferred = false;
    this.render_surface_pitch = 0;
    this.render_surface_offset = 0;
    this.render_surface_bpp = 0;
    this["debug_render_mode"] = {
        "width": this.render_width,
        "height": this.render_height,
        "bpp": this.render_bpp,
        "stride": this.render_stride,
        "offset": this.render_offset,
        "source": this.render_source,
        "surface": false,
        "tile": false,
        "tile_pitch": 0,
    };
    this.render_dirty_min = this.render_offset + this.render_frame_size;
    this.render_dirty_max = this.render_offset;
    this.render_dirty_rows = null;
    this.render_dirty_row_min = this.render_height;
    this.render_dirty_row_max = 0;

    if(this.screen && this.screen.set_mode)
    {
        this.screen.set_mode(false);
    }

    this.mc_soft_intr = false;
    this.mc_intr_en = 0;
    this.mc_enable = 0;
    this.update_irq_level();

    this.bus_intr = 0;
    this.bus_intr_en = 0;

    this.fifo_cache_error = 0;
    this.fifo_intr = 0;
    this.fifo_intr_en = 0;
    this.fifo_wait_notify = false;
    this.fifo_wait_flip = false;
    this.fifo_wait_soft = false;
    this.fifo_wait_acquire = false;
    this.fifo_wait_log_count = 0;
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
    this.fifo_last_dma_instance = 0;
    this.fifo_channels = [];
    this.fifo_subchannels = new Array(8);
    this.fifo_cache_method.fill(0);
    this.fifo_cache_data.fill(0);
    this.fifo_method_log_count = 0;

    this.timer_intr = 0;
    this.timer_intr_en = 0;
    this.timer_num = 0;
    this.timer_den = 0;
    this.timer_alarm = 0;
    this.timer_base_low = 0;
    this.timer_base_high = 0;
    this.timer_epoch_ms = nv20_now_ms();

    this.video_intr = 0;
    this.video_intr_en = 0;
    this.video_stop = 0;
    this.pvideo_regs.clear();

    this.straps0_primary = this.straps0_primary_original;
    this.fb_boot_0 = NV20_PMC_BOOT_0;
    this.fb_cfg = NV20_PFB_CFG0;
    this.fb_cfg0 = NV20_PFB_CFG0;

    this.graph_intr = 0;
    this.graph_intr_en = 0;
    this.graph_nsource = 0;
    this.graph_ctx_control = 0;
    this.graph_ctx_user = 0;
    this.graph_ctx_switch1 = 0;
    this.graph_ctx_switch2 = 0;
    this.graph_ctx_switch3 = 0;
    this.graph_ctx_switch4 = 0;
    this.graph_ctxctl_cur = 0;
    this.graph_status = 0;
    this.graph_trapped_addr = 0;
    this.graph_trapped_data = 0;
    this.graph_trapped_data_high = 0;
    this.graph_flip_read = 0;
    this.graph_flip_write = 0;
    this.graph_flip_modulo = 0;
    this.graph_notify = 0;
    this.graph_notify_instance = 0;
    this.graph_fifo = 0;
    this.graph_bpixel = 0;
    this.graph_channel_ctx_table = 0;
    this.graph_offset0 = 0;
    this.graph_pitch0 = 0;
    this.fb_tile0_flags = 0;
    this.fb_tile0_limit = 0;
    this.fb_tile0_pitch = 0;
    this.graph_object_states.clear();
    this["graph_accel_count"] = 0;
    this["graph_d3d_method_count"] = 0;
    this["graph_unhandled_method_count"] = 0;
    this["graph_last_unhandled_method"] = null;
    this["graph_unhandled_log_count"] = 0;
    this["graph_mono_upload_count"] = 0;
    this["graph_debug_2d"] = nv20_graph_debug_2d_default();

    this.crtc_intr = 0;
    this.crtc_intr_en = 0;
    this.crtc_start = 0;
    this.crtc_config = 0;
    this.crtc_raster_pos = 0;
    this.crtc_raster_counter = 0;
    this.crtc_cursor_offset = 0;
    this.crtc_cursor_config = 0;
    this.crtc_gpio_ext = 0;
    this.crtc_engine_ctrl = 0;
    this.crtc_status_read_count = 0;
    this.prmcio_crtc_index = 0;
    this.prmcio_crtc_regs.fill(0);
    nv20_init_default_crtc_regs(this.prmcio_crtc_regs);
    this.rma_addr = 0;
    this.reset_legacy_vga_shadow();
    this.reset_dispi_shadow();

    this.ramdac_cursor_start = 0;
    this.hw_cursor_enabled = false;
    this.hw_cursor_vram = true;
    this.hw_cursor_bpp32 = false;
    this.hw_cursor_size = 32;
    this.hw_cursor_offset = 0;
    this.hw_cursor_x = 0;
    this.hw_cursor_y = 0;
    this["debug_hw_cursor"] = null;
    this.ramdac_vpll = 0;
    this.ramdac_pll_select = 0;
    this.ramdac_vpll_b = 0;
    this.ramdac_general_control = 0;
    this.ramdac_fp_tg_control = 0;
    this.ramdac_dacclk = 0;

    if(this.option_rom)
    {
        this.pci_rom_enabled = false;
    }
};

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

NV20GeForce.prototype.update_fast_lfb_mapping = function()
{
    const fn = this.cpu && this.cpu.wm && this.cpu.wm.exports &&
        this.cpu.wm.exports["set_geforce_lfb_address"];

    if(!fn)
    {
        return;
    }

    fn(this.vram_fast_memory ? this.vram_base >>> 0 : 0);
};

NV20GeForce.prototype.vram_fast_array = function(offset, width)
{
    offset >>>= 0;
    width >>>= 0;

    if(this.vram_fast_memory &&
        offset + width <= this.vram_fast_size)
    {
        return this.vram_fast_memory;
    }

    return this.vram;
};

NV20GeForce.prototype.vram_note_fast_dirty = function(offset, width)
{
    if(!this.vram_fast_memory || !width)
    {
        return;
    }

    offset >>>= 0;
    width >>>= 0;
    const end = Math.min(this.vram_fast_size, offset + width);

    if(offset >= end)
    {
        return;
    }

    if(offset < this.vram_fast_dirty_min)
    {
        this.vram_fast_dirty_min = offset;
    }

    if(end > this.vram_fast_dirty_max)
    {
        this.vram_fast_dirty_max = end;
    }

    this.vram_fast_dirty = true;
};

NV20GeForce.prototype.vram_sync_fast_range = function(offset, width)
{
    if(!this.vram_fast_memory || !width)
    {
        return;
    }

    offset >>>= 0;
    width >>>= 0;
    const end = Math.min(this.vram_fast_size, this.vram.length, offset + width);

    if(offset >= end)
    {
        return;
    }

    this.vram_fast_memory.set(this.vram.subarray(offset, end), offset);
    this.vram_note_fast_dirty(offset, end - offset);
};

NV20GeForce.prototype.vram_read8 = function(offset)
{
    offset = this.vram_offset(offset);
    return this.vram_fast_array(offset, 1)[offset];
};

NV20GeForce.prototype.vram_write8 = function(offset, value)
{
    offset = this.vram_offset(offset);
    value &= 0xFF;

    this.vram[offset] = value;

    if(this.vram_fast_memory && offset < this.vram_fast_size)
    {
        this.vram_fast_memory[offset] = value;
        this.vram_note_fast_dirty(offset, 1);
    }

    this.vram_mark_write(offset, 1, value);
};

NV20GeForce.prototype.vram_read32 = function(offset)
{
    offset = this.vram_offset(offset);

    const vram = this.vram_fast_array(offset, 4);

    if(offset + 3 < vram.length)
    {
        return (vram[offset] |
                vram[offset + 1] << 8 |
                vram[offset + 2] << 16 |
                vram[offset + 3] << 24) >>> 0;
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

    for(var i = 0; i < 4; i++)
    {
        const dst = this.vram_offset(offset + i);
        const byte = value >>> (i << 3) & 0xFF;
        this.vram[dst] = byte;

        if(this.vram_fast_memory && dst < this.vram_fast_size)
        {
            this.vram_fast_memory[dst] = byte;
            this.vram_note_fast_dirty(dst, 1);
        }
    }

    this.vram_mark_write(offset, 4, value);
};

NV20GeForce.prototype.render_clear_dirty = function()
{
    this.render_dirty_min = this.render_offset + this.render_frame_size;
    this.render_dirty_max = this.render_offset;
    this.render_dirty_row_min = this.render_height;
    this.render_dirty_row_max = 0;

    if(this.render_dirty_rows)
    {
        this.render_dirty_rows.fill(0);
    }
};

NV20GeForce.prototype.render_mark_dirty_rows = function(dirty_start, dirty_end)
{
    if(dirty_start >= dirty_end || !this.render_stride)
    {
        return;
    }

    const dirty_min = Math.max(0, dirty_start - this.render_offset);
    const dirty_max = Math.min(this.render_frame_size, dirty_end - this.render_offset);

    if(dirty_min >= dirty_max)
    {
        return;
    }

    const min_y = Math.max(0, Math.min(this.render_height, dirty_min / this.render_stride | 0));
    const max_y = Math.max(min_y, Math.min(this.render_height,
        (dirty_max + this.render_stride - 1) / this.render_stride | 0));

    if(min_y >= max_y)
    {
        return;
    }

    if(!this.render_dirty_rows || this.render_dirty_rows.length !== this.render_height)
    {
        this.render_dirty_rows = new Uint8Array(this.render_height);
    }

    for(var y = min_y; y < max_y; y++)
    {
        this.render_dirty_rows[y] = 1;
    }

    if(min_y < this.render_dirty_row_min)
    {
        this.render_dirty_row_min = min_y;
    }

    if(max_y > this.render_dirty_row_max)
    {
        this.render_dirty_row_max = max_y;
    }
};

NV20GeForce.prototype.render_mark_dirty_rect = function(x, y, width, height)
{
    if(!this.render_stride || !this.render_frame_size)
    {
        return;
    }

    x |= 0;
    y |= 0;
    width = width >>> 0;
    height = height >>> 0;

    const x0 = Math.max(0, x);
    const y0 = Math.max(0, y);
    const x1 = Math.min(this.render_width, x + width);
    const y1 = Math.min(this.render_height, y + height);

    if(x0 >= x1 || y0 >= y1)
    {
        return;
    }

    const bytes_per_pixel = nv20_render_bytes_per_pixel(this.render_bpp);
    const dirty_start = this.render_offset + y0 * this.render_stride + x0 * bytes_per_pixel;
    const dirty_end = this.render_offset + (y1 - 1) * this.render_stride + x1 * bytes_per_pixel;

    if(dirty_start < this.render_dirty_min)
    {
        this.render_dirty_min = dirty_start;
    }

    if(dirty_end > this.render_dirty_max)
    {
        this.render_dirty_max = dirty_end;
    }

    this.render_mark_dirty_rows(dirty_start, dirty_end);
    this.activate_rendering();
    this.schedule_render();
};

NV20GeForce.prototype.hw_cursor_mark_dirty = function(x, y, size)
{
    this.render_mark_dirty_rect(x, y, size, size);
};

NV20GeForce.prototype.hw_cursor_read16 = function(offset)
{
    offset >>>= 0;

    if(this.hw_cursor_vram)
    {
        return this.vram_read8(offset) | this.vram_read8(offset + 1) << 8;
    }

    const word = this.ramin_read32(offset & ~3);
    return word >>> ((offset & 2) << 3) & 0xFFFF;
};

NV20GeForce.prototype.hw_cursor_read32 = function(offset)
{
    offset >>>= 0;

    if(this.hw_cursor_vram)
    {
        return this.vram_read32(offset);
    }

    return this.ramin_read32(offset);
};

NV20GeForce.prototype.hw_cursor_update = function(mark_dirty)
{
    const old_enabled = this.hw_cursor_enabled;
    const old_x = this.hw_cursor_x;
    const old_y = this.hw_cursor_y;
    const old_size = this.hw_cursor_size;
    const crtc = this.prmcio_crtc_regs;

    this.hw_cursor_enabled = !!((crtc[0x31] & 1) || (this.crtc_cursor_config & 1));
    this.hw_cursor_vram = !!((crtc[0x30] & 0x80) || (this.crtc_cursor_config & 0x00000100));
    this.hw_cursor_bpp32 = !!(this.crtc_cursor_config & 0x00001000);
    this.hw_cursor_size = this.crtc_cursor_config & 0x00010000 ? 64 : 32;
    this.hw_cursor_offset = (
        (crtc[0x31] >>> 2 << 11) |
        (crtc[0x30] & 0x7F) << 17 |
        crtc[0x2F] << 24
    ) + this.crtc_cursor_offset >>> 0;
    this.hw_cursor_x = nv20_sign_extend(this.ramdac_cursor_start & 0x0FFF, 12);
    this.hw_cursor_y = nv20_sign_extend(this.ramdac_cursor_start >>> 16 & 0x0FFF, 12);
    this["debug_hw_cursor"] = {
        "enabled": this.hw_cursor_enabled,
        "vram": this.hw_cursor_vram,
        "bpp32": this.hw_cursor_bpp32,
        "size": this.hw_cursor_size,
        "offset": this.hw_cursor_offset >>> 0,
        "x": this.hw_cursor_x,
        "y": this.hw_cursor_y,
        "config": this.crtc_cursor_config >>> 0,
        "pos": this.ramdac_cursor_start >>> 0,
        "crtc2f": crtc[0x2F],
        "crtc30": crtc[0x30],
        "crtc31": crtc[0x31],
    };

    if(!mark_dirty)
    {
        return;
    }

    if(old_enabled)
    {
        this.hw_cursor_mark_dirty(old_x, old_y, old_size);
    }

    if(this.hw_cursor_enabled)
    {
        this.hw_cursor_mark_dirty(this.hw_cursor_x, this.hw_cursor_y, this.hw_cursor_size);
    }
};

NV20GeForce.prototype.render_overlay_hw_cursor = function(min_y, max_y)
{
    if(!this.hw_cursor_enabled || !this.render_buffer)
    {
        return;
    }

    const size = this.hw_cursor_size | 0;
    const cursor_x = this.hw_cursor_x | 0;
    const cursor_y = this.hw_cursor_y | 0;
    const x0 = Math.max(0, cursor_x);
    const y0 = Math.max(min_y, Math.max(0, cursor_y));
    const x1 = Math.min(this.render_width, cursor_x + size);
    const y1 = Math.min(max_y, Math.min(this.render_height, cursor_y + size));

    if(x0 >= x1 || y0 >= y1)
    {
        return;
    }

    const dst = this.render_buffer;
    const cursor_bytes = this.hw_cursor_bpp32 ? 4 : 2;
    const cursor_pitch = size * cursor_bytes;

    for(var y = y0; y < y1; y++)
    {
        var cursor_i = this.hw_cursor_offset +
            (y - cursor_y) * cursor_pitch +
            (x0 - cursor_x) * cursor_bytes >>> 0;
        var dst_i = (y * this.render_width + x0) * 4;

        for(var x = x0; x < x1; x++, cursor_i += cursor_bytes, dst_i += 4)
        {
            if(this.hw_cursor_bpp32)
            {
                const color = this.hw_cursor_read32(cursor_i);
                const alpha = color >>> 24;

                if(alpha)
                {
                    const inverse_alpha = 0xFF - alpha;
                    dst[dst_i] = nv20_clamp8((dst[dst_i] * inverse_alpha + (color >> 16 & 0xFF) * alpha) / 0xFF);
                    dst[dst_i + 1] = nv20_clamp8((dst[dst_i + 1] * inverse_alpha + (color >> 8 & 0xFF) * alpha) / 0xFF);
                    dst[dst_i + 2] = nv20_clamp8((dst[dst_i + 2] * inverse_alpha + (color & 0xFF) * alpha) / 0xFF);
                    dst[dst_i + 3] = 0xFF;
                }
            }
            else
            {
                const color = this.hw_cursor_read16(cursor_i);

                if(color & 0x8000)
                {
                    const r = (color >> 10 & 0x1F) * 0xFF / 0x1F | 0;
                    const g = (color >> 5 & 0x1F) * 0xFF / 0x1F | 0;
                    const b = (color & 0x1F) * 0xFF / 0x1F | 0;
                    dst[dst_i] = r;
                    dst[dst_i + 1] = g;
                    dst[dst_i + 2] = b;
                    dst[dst_i + 3] = 0xFF;
                }
            }
        }
    }
};

NV20GeForce.prototype.on_vga_palette_change = function()
{
    if(this.render_bpp !== 8 || !this.render_active)
    {
        return;
    }

    this.render_mark_dirty_rect(0, 0, this.render_width, this.render_height);
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

NV20GeForce.prototype.bar2_read32 = function(offset)
{
    return this.ramin_read32(offset & (NV20_BAR2_SIZE - 1));
};

NV20GeForce.prototype.bar2_write32 = function(offset, value)
{
    offset = offset & (NV20_BAR2_SIZE - 1) & ~3;
    this.ramin_write32(offset, value);
    this.fifo_note_pramin_write(offset);
};

NV20GeForce.prototype.bar2_read8 = function(offset)
{
    const aligned_offset = offset & ~3;
    return this.bar2_read32(aligned_offset) >>> ((offset & 3) << 3) & 0xFF;
};

NV20GeForce.prototype.bar2_write8 = function(offset, value)
{
    const aligned_offset = offset & ~3;
    const shift = (offset & 3) << 3;
    const mask = 0xFF << shift;
    const old_value = this.bar2_read32(aligned_offset);
    this.bar2_write32(aligned_offset, old_value & ~mask | (value & 0xFF) << shift);
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
        last_dma_instance: 0,
        dma_state: 0x80000000,
        dma_fetch: 0,
        dma_dcount: 0,
        dma_subroutine: 0,
        dma_subroutine_active: false,
        context_loaded: false,
        processing: false,
        dma_kick_scheduled: false,
        pending_method: 0,
        pending_count: 0,
        pending_subchannel: 0,
        pending_non_increasing: false,
        subchannels: new Array(8),
    };

    this.fifo_channels[chid] = channel;
    return channel;
};

NV20GeForce.prototype.fifo_reset_channel_state = function(channel, clear_objects)
{
    channel.dma_put = 0;
    channel.dma_get = 0;
    channel.dma_put_high = 0;
    channel.dma_get_high = 0;
    channel.dma_mget = 0;
    channel.dma_mget_high = 0;
    channel.dma_cget = 0;
    channel.ref = 0;
    channel.dma_instance = 0;
    if(clear_objects)
    {
        channel.last_dma_instance = 0;
    }
    channel.dma_state = 0x80000000;
    channel.dma_fetch = 0;
    channel.dma_dcount = 0;
    channel.dma_subroutine = 0;
    channel.dma_subroutine_active = false;
    channel.context_loaded = false;
    channel.processing = false;
    channel.dma_kick_scheduled = false;
    channel.pending_method = 0;
    channel.pending_count = 0;
    channel.pending_subchannel = 0;
    channel.pending_non_increasing = false;

    if(clear_objects)
    {
        channel.subchannels = new Array(8);
    }
};

NV20GeForce.prototype.fifo_reset_all_channel_state = function(clear_objects)
{
    for(var i = 0; i < this.fifo_channels.length; i++)
    {
        if(this.fifo_channels[i])
        {
            this.fifo_reset_channel_state(this.fifo_channels[i], clear_objects);
        }
    }

    if(clear_objects)
    {
        this.fifo_subchannels = new Array(8);
        this.graph_object_states.clear();
    }

    this.fifo_dma_put = 0;
    this.fifo_dma_get = 0;
    this.fifo_dma_instance = 0;
    if(clear_objects)
    {
        this.fifo_last_dma_instance = 0;
    }
    this.fifo_dma_dcount = 0;
    this.fifo_dma_state = 0x80000000;
    this.fifo_dma_fetch = 0;
    this.fifo_dma_push = 0;
    this.fifo_ref_cnt = 0;
};

NV20GeForce.prototype.fifo_note_pramin_write = function(offset)
{
    if(!this.fifo_ramfc)
    {
        return;
    }

    const info = this.fifo_ramfc_info();
    const relative = offset - info.base;

    if(relative < 0 || relative >= 32 * info.stride)
    {
        return;
    }

    const chid = relative / info.stride | 0;
    const channel = this.fifo_channels[chid];

    if(channel)
    {
        channel.context_loaded = false;
    }
};

NV20GeForce.prototype.fifo_active_channel_state = function()
{
    return this.fifo_channel(this.fifo_active_channel);
};

NV20GeForce.prototype.fifo_set_active_channel = function(chid)
{
    const old_channel = this.fifo_active_channel_state();
    const channel = this.fifo_channel(chid);

    if(old_channel.id !== channel.id && old_channel.context_loaded)
    {
        this.fifo_save_channel_context(old_channel);
    }

    this.fifo_active_channel = channel.id;
    this.fifo_cache1_push1 = this.fifo_cache1_push1 & ~0x1F | channel.id;
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

NV20GeForce.prototype.fifo_has_dma_push_work = function()
{
    if(this.fifo_should_wait())
    {
        return true;
    }

    for(var i = 0; i < this.fifo_channels.length; i++)
    {
        const channel = this.fifo_channels[i];

        if(!channel || !this.fifo_is_dma_channel(channel.id))
        {
            continue;
        }

        if(channel.processing ||
           channel.pending_count ||
           channel.dma_get !== channel.dma_put)
        {
            return true;
        }
    }

    return false;
};

NV20GeForce.prototype.fifo_update_dma_push_state = function()
{
    if(!this.fifo_has_dma_push_work())
    {
        this.fifo_intr &= ~NV20_FIFO_INTR_DMA_PUSHER;
        this.update_irq_level();
    }
};

NV20GeForce.prototype.fifo_dma_push_read = function()
{
    const work = this.fifo_has_dma_push_work();
    return (this.fifo_dma_push & NV20_FIFO_DMA_PUSH_CONTROL_MASK) |
        (work ? NV20_FIFO_DMA_PUSH_STATE_BUSY : NV20_FIFO_DMA_PUSH_BUFFER_EMPTY);
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
        // NV20 uses the pre-NV40 RAMFC encoding: low 12 bits are the base
        // page, each channel context is 0x40 bytes.
        base: (this.fifo_ramfc & 0xFFF) << 8 & (NV20_PRAMIN_SIZE - 1),
        stride: NV20_RAMFC_NV10_STRIDE,
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

    const offset = this.fifo_ramfc_offset(channel.id);

    channel.dma_put = this.ramin_read32(offset + 0x00);
    channel.dma_get = this.ramin_read32(offset + 0x04);
    channel.ref = this.ramin_read32(offset + 0x08);
    channel.dma_instance = this.ramin_read32(offset + 0x0C) & 0xFFFF;
    channel.semaphore = this.ramin_read32(offset + 0x2C);
    channel.dma_state = channel.pending_count ? 0 : 0x80000000;
    channel.dma_dcount = channel.pending_count >>> 0;

    this.fifo_sync_cache1_from_channel(channel);
};

NV20GeForce.prototype.fifo_save_channel_context = function(channel)
{
    if(!this.fifo_ramfc)
    {
        return;
    }

    const offset = this.fifo_ramfc_offset(channel.id);

    this.ramin_write32(offset + 0x00, channel.dma_put);
    this.ramin_write32(offset + 0x04, channel.dma_get);
    this.ramin_write32(offset + 0x08, channel.ref);
    this.ramin_write32(offset + 0x0C, channel.dma_instance);
    this.ramin_write32(offset + 0x2C, channel.semaphore || 0);
};

NV20GeForce.prototype.fifo_hash_handle = function(handle, chid, entry_bits)
{
    const mask = (1 << entry_bits) - 1;
    var hash = 0;
    var value = handle >>> 0;

    while(value)
    {
        hash ^= value & mask;
        value >>>= entry_bits;
    }

    hash ^= (chid & 0xF) << (entry_bits - 4);
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

        if(context === 0)
        {
            continue;
        }

        const context_channel = context >>> 24 & 0x1F;

        if(context_channel !== (chid & 0x1F))
        {
            continue;
        }

        const instance = (context & 0xFFFF) << 4 & (NV20_PRAMIN_SIZE - 1);

        if(!instance)
        {
            continue;
        }

        const class_id = nv20_object_class_from_word(this.ramin_read32(instance));

        if(!class_id)
        {
            continue;
        }

        return {
            handle: handle,
            context: context >>> 0,
            instance: instance >>> 0,
            engine: context >>> 16 & 0xFF,
            channel: context_channel,
            class_id: class_id,
            index: index,
        };
    }

    return null;
};

NV20GeForce.prototype.fifo_read_system32 = function(address)
{
    address >>>= 0;

    const mem8 = this.cpu && this.cpu.mem8;

    if(!mem8 || address + 3 >= mem8.length)
    {
        return null;
    }

    return (mem8[address] |
            mem8[address + 1] << 8 |
            mem8[address + 2] << 16 |
            mem8[address + 3] << 24) >>> 0;
};

NV20GeForce.prototype.dma_translate = function(dma, offset)
{
    if(!dma)
    {
        return null;
    }

    offset >>>= 0;

    var address;

    if(dma.linear)
    {
        address = dma.base + dma.adjust + offset >>> 0;
    }
    else
    {
        const adjusted = dma.adjust + offset >>> 0;
        const page_index = adjusted >>> 12;
        const page_offset = adjusted & 0xFFF;
        const page = this.ramin_read32(dma.offset + 8 + page_index * 4) & 0xFFFFF000;
        address = page | page_offset;
    }

    return {
        address: address >>> 0,
        physical: dma.physical,
    };
};

NV20GeForce.prototype.fifo_dma_object = function(channel)
{
    const instance = (channel.dma_instance ||
                      this.fifo_dma_instance ||
                      channel.last_dma_instance ||
                      this.fifo_last_dma_instance) & 0xFFFF;

    if(!instance)
    {
        return null;
    }

    if(!channel.dma_instance)
    {
        channel.dma_instance = instance;
    }

    channel.last_dma_instance = instance;
    this.fifo_last_dma_instance = instance;

    const offset = instance << 4 & (NV20_PRAMIN_SIZE - 1);
    const flags = this.ramin_read32(offset);
    const limit = this.ramin_read32(offset + 4);
    const base_raw = this.ramin_read32(offset + 8);

    return {
        instance: instance,
        offset: offset,
        flags: flags,
        limit: limit,
        base_raw: base_raw >>> 0,
        base: base_raw & 0xFFFFF000,
        target: base_raw & 3,
        adjust: flags >>> 20,
        linear: !!(flags & 0x00002000),
        physical: !!(flags & 0x00020000),
    };
};

NV20GeForce.prototype.fifo_find_dma_push_instance = function(channel)
{
    const min_limit = Math.max(channel.dma_put >>> 0, channel.dma_get >>> 0);
    var best_instance = 0;
    var best_limit = 0;

    for(var offset = 0; offset < NV20_PRAMIN_SIZE; offset += 0x10)
    {
        const flags = this.ramin_read32(offset);

        if((flags & 0xFFF) !== NV20_CLASS_DMA_IN_MEMORY)
        {
            continue;
        }

        if(!(flags & 0x00020000))
        {
            continue;
        }

        const limit = this.ramin_read32(offset + 4) >>> 0;

        if(limit < min_limit || limit < 0x1000 || limit <= best_limit)
        {
            continue;
        }

        const base = this.ramin_read32(offset + 8);

        if((base & 3) !== 3)
        {
            continue;
        }

        best_instance = offset >>> 4;
        best_limit = limit;
    }

    return best_instance & 0xFFFF;
};

NV20GeForce.prototype.fifo_recover_dma_instance = function(channel, source)
{
    if((channel.dma_instance & 0xFFFF) || (this.fifo_dma_instance & 0xFFFF))
    {
        return true;
    }

    if((channel.last_dma_instance & 0xFFFF) || (this.fifo_last_dma_instance & 0xFFFF))
    {
        channel.dma_instance = (channel.last_dma_instance || this.fifo_last_dma_instance) & 0xFFFF;
        this.fifo_sync_cache1_from_channel(channel);
        return true;
    }

    const instance = this.fifo_find_dma_push_instance(channel);

    if(!instance)
    {
        return false;
    }

    channel.dma_instance = instance;
    channel.last_dma_instance = instance;
    this.fifo_last_dma_instance = instance;
    channel.context_loaded = true;
    this.fifo_sync_cache1_from_channel(channel);

    if(this.fifo_trace)
    {
        dbg_log(this.name + " pfifo recovered dma instance channel=" + channel.id +
                " instance=" + h(instance, 4) +
                " get=" + h(channel.dma_get >>> 0, 8) +
                " put=" + h(channel.dma_put >>> 0, 8) +
                " source=" + (source || "unknown"), LOG_PCI);
    }

    return true;
};

NV20GeForce.prototype.fifo_is_dma_channel = function(chid)
{
    chid &= 0x1F;
    return !!((this.fifo_mode >>> 0) & (1 << chid));
};

NV20GeForce.prototype.fifo_is_dma_user_reg = function(reg)
{
    switch(reg & 0x1FFC)
    {
        case NV20_FIFO_DMA_PUT:
        case NV20_FIFO_DMA_GET:
        case NV20_FIFO_REF:
        case NV20_FIFO_DMA_PUT_HIGH:
        case NV20_FIFO_DMA_CGET:
        case NV20_FIFO_DMA_MGET:
        case NV20_FIFO_DMA_MGET_HIGH:
        case NV20_FIFO_DMA_GET_HIGH:
            return true;
        default:
            return false;
    }
};

NV20GeForce.prototype.fifo_dma_read32 = function(channel, offset, source)
{
    offset >>>= 0;

    const object = this.fifo_dma_object(channel);
    var value = null;

    if(object)
    {
        const translated = this.dma_translate(object, offset);
        if(translated)
        {
            if(translated.physical)
            {
                value = this.fifo_read_system32(translated.address);
            }
            else
            {
                value = this.vram_read32(translated.address % this.vram_size);
            }
        }
    }

    if(value === null)
    {
        if(object)
        {
            channel.dma_state = 0x80000000;
            this.fifo_intr |= NV20_FIFO_INTR_DMA_PTE;
            this.update_irq_level();
        }

        if(this.fifo_trace)
        {
            dbg_log(this.name + " pfifo dma read failed channel=" + channel.id +
                    " get=" + h(offset, 8) +
                    " put=" + h(channel.dma_put >>> 0, 8) +
                    " source=" + (source || "unknown") +
                    (object ? " instance=" + h(object.instance, 4) +
                              " base=" + h(object.base, 8) +
                              " limit=" + h(object.limit, 8) +
                              " flags=" + h(object.flags, 8) :
                              " instance=0"), LOG_PCI);
        }
    }

    return value;
};

NV20GeForce.prototype.fifo_dma_reg_read32 = function(channel, reg)
{
    this.fifo_load_channel_context(channel);

    switch(reg & 0x1FFC)
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

NV20GeForce.prototype.fifo_dma_reg_write32 = function(channel, reg, value)
{
    value >>>= 0;
    reg &= 0x1FFC;

    this.fifo_load_channel_context(channel);

    switch(reg)
    {
        case NV20_FIFO_DMA_PUT:
            channel.dma_put = value;
            this.fifo_set_active_channel(channel.id);
            this.fifo_dma_kick(channel, "user-put", NV20_FIFO_DMA_SYNC_KICK_LIMIT);
            return true;
        case NV20_FIFO_DMA_GET:
            channel.dma_get = value;
            this.fifo_set_active_channel(channel.id);
            this.fifo_sync_cache1_from_channel(channel);
            return true;
        case NV20_FIFO_REF:
            channel.ref = value;
            this.fifo_sync_cache1_from_channel(channel);
            return true;
        case NV20_FIFO_DMA_PUT_HIGH:
            channel.dma_put_high = value;
            return true;
        case NV20_FIFO_DMA_CGET:
            channel.dma_cget = value;
            return true;
        case NV20_FIFO_DMA_MGET:
            channel.dma_mget = value;
            return true;
        case NV20_FIFO_DMA_MGET_HIGH:
            channel.dma_mget_high = value;
            return true;
        case NV20_FIFO_DMA_GET_HIGH:
            channel.dma_get_high = value;
            return true;
        default:
            return false;
    }
};

NV20GeForce.prototype.fifo_kick_enabled_dma_channels = function(source)
{
    if(this.fifo_should_wait())
    {
        return;
    }

    if((this.fifo_cache1_push0 & 1) === 0 || (this.fifo_pull0 & 1) === 0)
    {
        this.fifo_update_dma_push_state();
        return;
    }

    const start = (this.fifo_cache1_push1 & 0x1F) + 1;

    for(var index = 0; index < 32; index++)
    {
        const chid = start + index & 0x1F;

        if(this.fifo_is_dma_channel(chid))
        {
            const channel = this.fifo_channel(chid);
            this.fifo_load_channel_context(channel);

            if(channel.dma_put !== channel.dma_get || channel.pending_count)
            {
                this.fifo_set_active_channel(chid);
                this.fifo_dma_kick(channel, source, NV20_FIFO_DMA_SYNC_KICK_LIMIT);
            }
        }
    }

    this.fifo_update_dma_push_state();
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

NV20GeForce.prototype.fifo_should_wait = function()
{
    return this.fifo_wait_notify ||
        this.fifo_wait_flip ||
        this.fifo_wait_soft ||
        this.fifo_wait_acquire;
};

NV20GeForce.prototype.fifo_note_wait = function(reason)
{
    if(this.fifo_wait_log_count++ < 16)
    {
        dbg_log(this.name + " pfifo wait " + reason +
                " notify=" + (this.fifo_wait_notify ? 1 : 0) +
                " flip=" + (this.fifo_wait_flip ? 1 : 0) +
                " acquire=" + (this.fifo_wait_acquire ? 1 : 0) +
                " read=" + h(this.graph_flip_read >>> 0, 8) +
                " write=" + h(this.graph_flip_write >>> 0, 8) +
                " modulo=" + h(this.graph_flip_modulo >>> 0, 8), LOG_PCI);
    }
};

NV20GeForce.prototype.fifo_resume_wait = function(reason)
{
    if(this.fifo_wait_log_count++ < 16)
    {
        dbg_log(this.name + " pfifo resume " + reason +
                " read=" + h(this.graph_flip_read >>> 0, 8) +
                " write=" + h(this.graph_flip_write >>> 0, 8), LOG_PCI);
    }

    this.fifo_kick_enabled_dma_channels("wait-" + reason);
};

NV20GeForce.prototype.log_missing_command = function(kind, key, detail)
{
    if(!this.missing_trace)
    {
        return;
    }

    key = kind + ":" + key;

    var entry = this.missing_commands.get(key);

    if(entry)
    {
        entry.count++;
        entry.last = detail;
    }
    else
    {
        entry = {
            count: 1,
            kind: kind,
            first: detail,
            last: detail,
        };
        this.missing_commands.set(key, entry);
    }

    const debug = this["debug_missing_commands"];
    debug.total++;
    debug.unique = this.missing_commands.size;
    debug.last = {
        kind: kind,
        key: key,
        count: entry.count,
        detail: detail,
    };

    if(entry.count > 4 && (entry.count & entry.count - 1) !== 0)
    {
        return;
    }

    if(this.missing_log_count >= this.missing_log_limit)
    {
        if(!this.missing_log_suppressed)
        {
            this.missing_log_suppressed = true;
            dbg_log(this.name + " missing command log suppressed" +
                    " unique=" + this.missing_commands.size +
                    " total=" + debug.total, LOG_PCI);
        }

        return;
    }

    this.missing_log_count++;
    dbg_log(this.name + " missing command kind=" + kind +
            " count=" + entry.count + " " + detail, LOG_PCI);
};

NV20GeForce.prototype.graph_log_unhandled_method = function(channel, subchannel, method, data, source)
{
    const object = channel.subchannels[subchannel] || this.fifo_subchannels[subchannel];
    const state = object && object.state;
    const class_id = state ? state.class_id : object ? object.class_id : 0;
    const handle = object ? object.handle : 0;
    const class_name = nv20_class_name(class_id);
    const key = "ch" + channel.id +
        ":subc" + subchannel +
        ":class" + h(class_id >>> 0, 4) +
        ":method" + h(method >>> 0, 4);

    this["graph_unhandled_log_count"]++;
    this.log_missing_command("pfifo-method", key,
        "channel=" + channel.id +
        " subc=" + subchannel +
        " class=" + h(class_id >>> 0, 4) +
        (class_name ? " (" + class_name + ")" : "") +
        " handle=" + h(handle >>> 0, 8) +
        " method=" + h(method >>> 0, 4) +
        " data=" + h(data >>> 0, 8) +
        " source=" + (source || "method"));
};

NV20GeForce.prototype.graph_object_key = function(channel, object)
{
    if(object && object.instance)
    {
        return "i:" + h(object.instance >>> 0, 5);
    }

    return "h:" + channel.id + ":" + h(object && object.handle || 0, 8);
};

NV20GeForce.prototype.graph_default_surface = function()
{
    return {
        class_id: NV20_CLASS_SURFACE_2D_NV10,
        class_name: "default-surface",
        format: nv20_surface_format_from_bpp(this.render_bpp),
        src_pitch: this.render_stride,
        dst_pitch: this.render_stride,
        src_offset: this.render_offset,
        dst_offset: this.render_offset,
        surface_src_offset_set: true,
        surface_dst_offset_set: true,
        src_dma: null,
        dst_dma: null,
    };
};

NV20GeForce.prototype.graph_init_object_state = function(channel, object)
{
    const class_id = object && object.class_id || 0;
    const instance = object && object.instance || 0;
    var notifier_dma = null;

    if(instance)
    {
        const object_offset = instance & (NV20_PRAMIN_SIZE - 1) & ~0xF;
        const notifier_instance = (this.ramin_read32(object_offset + 4) >>> 16) << 4;

        if(notifier_instance)
        {
            notifier_dma = this.graph_dma_object_from_instance(notifier_instance, 0, 0);
        }
    }

    return {
        channel_id: channel.id,
        handle: object && object.handle || 0,
        instance: instance,
        class_id: class_id,
        class_name: nv20_class_name(class_id),
        operation: NV20_2D_OPERATION_SRCCOPY,
        color_format: 0,
        color_format_set: false,
        ifc_indexed_format: false,
        color: 0,
        format: nv20_surface_format_from_bpp(this.render_bpp),
        surface_format_set: false,
        surface_pitch_set: false,
        surface_src_offset_set: false,
        surface_dst_offset_set: false,
        src_pitch: this.render_stride,
        dst_pitch: this.render_stride,
        src_offset: this.render_offset,
        dst_offset: this.render_offset,
        src_dma: null,
        dst_dma: null,
        notifier_dma: notifier_dma,
        notifier_handle: 0,
        notify_pending: false,
        notify_type: 0,
        surface: null,
        clip: null,
        pattern: null,
        rop_context: null,
        chroma: null,
        beta_context: null,
        swizzled_surface: null,
        clip_point: { x: 0, y: 0 },
        clip_size: { w: 0, h: 0 },
        point: { x: 0, y: 0 },
        point_in: { x: 0, y: 0 },
        point_out: { x: 0, y: 0 },
        size: { w: 0, h: 0 },
        size_in: { w: 0, h: 0 },
        size_out: { w: 0, h: 0 },
        rect_xy: { x: 0, y: 0 },
        rect_xy0: { x: 0, y: 0 },
        clip_xy0: { x: 0, y: 0 },
        clip_xy1: { x: 0, y: 0 },
        gdi_color_format: 0,
        gdi_mono_format: 0,
        gdi_bg_color: 0,
        gdi_fg_color: 0xFFFFFFFF,
        gdi_image_swh: { w: 0, h: 0 },
        gdi_image_dwh: { w: 0, h: 0 },
        gdi_image_xy: { x: 0, y: 0 },
        gdi_words: [],
        gdi_words_left: 0,
        gdi_words_ptr: 0,
        dx_du: 0,
        dy_dv: 0,
        point12d4: { x: 0, y: 0 },
        image_upload: null,
        iifc_palette_handle: 0,
        iifc_palette_dma: null,
        iifc_palette_offset: 0,
        iifc_operation: NV20_2D_OPERATION_SRCCOPY,
        iifc_color_format: 1,
        iifc_bpp4: false,
        iifc_yx: { x: 0, y: 0 },
        iifc_dhw: { w: 0, h: 0 },
        iifc_shw: { w: 0, h: 0 },
        iifc_words: null,
        iifc_words_ptr: 0,
        iifc_words_left: 0,
        m2mf_offset_in: 0,
        m2mf_offset_out: 0,
        m2mf_pitch_in: 0,
        m2mf_pitch_out: 0,
        m2mf_line_length: 0,
        m2mf_line_count: 0,
        m2mf_format: 0x00000101,
        m2mf_in_dma: null,
        m2mf_out_dma: null,
        m2mf_executed: false,
        rop: 0xCC,
        pattern_shape: 0,
        pattern_color_type: 0,
        pattern_bg_color: 0,
        pattern_fg_color: 0,
        pattern_mono: new Uint32Array(2),
        pattern_color: [],
        chroma_color_format: 0,
        chroma_color: 0,
        chroma_color_set: false,
        beta: 0xFFFFFFFF,
        swz_dma: null,
        swz_format: 0,
        swz_width: 1,
        swz_height: 1,
        swz_offset: 0,
        mono_format: 0,
        mono_color0: 0,
        mono_color1: 0xFFFFFFFF,
        tex_clip_point: { x: 0, y: 0 },
        tex_clip_size: { w: 0, h: 0 },
        sifm_src_dma: null,
        sifm_surface: null,
        sifm_color_format: 0,
        sifm_operation: NV20_2D_OPERATION_SRCCOPY,
        sifm_dyx: { x: 0, y: 0 },
        sifm_dhw: { w: 0, h: 0 },
        sifm_dudx: 0x00100000,
        sifm_dvdy: 0x00100000,
        sifm_shw: { w: 0, h: 0 },
        sifm_sfmt: 0,
        sifm_sofs: 0,
        sifm_syx: { x: 0, y: 0 },
        sifm_syx_raw: 0,
        d3d_a_dma: null,
        d3d_b_dma: null,
        d3d_color_dma: null,
        d3d_zeta_dma: null,
        d3d_vertex_a_dma: null,
        d3d_vertex_b_dma: null,
        d3d_semaphore_dma: null,
        d3d_report_dma: null,
        d3d_methods: {},
        d3d_textures: [],
        d3d_vertex_arrays: [],
        d3d_vertex_data_imm: [],
        d3d_transform_program: [],
        d3d_transform_program_load: 0,
        d3d_transform_program_start: 0,
        d3d_transform_constant: [],
        d3d_transform_constant_load: 0,
        d3d_attrib_count: 16,
        d3d_tex_coord_count: 8,
        d3d_window_offset_x: 0,
        d3d_window_offset_y: 0,
        d3d_window_clip: [],
        d3d_scissor_x: 0,
        d3d_scissor_y: 0,
        d3d_scissor_width: 0,
        d3d_scissor_height: 0,
        d3d_viewport_x: 0,
        d3d_viewport_y: 0,
        d3d_viewport_width: 0,
        d3d_viewport_height: 0,
        d3d_begin_end: 0,
        d3d_vertex_data_base_index: 0,
        d3d_index_array_offset: 0,
        d3d_index_array_dma: 0,
        d3d_semaphore_offset: 0,
        d3d_surface_format: 0,
        d3d_surface_pitch: 0,
        d3d_surface_pitch_z: 0,
        d3d_surface_color_offset: 0,
        d3d_surface_zeta_offset: 0,
        d3d_color_clear_value: 0,
        d3d_zstencil_clear_value: 0,
        d3d_clear_surface: 0,
        d3d_shader_control: 0,
    };
};

NV20GeForce.prototype.graph_get_object_state = function(channel, object)
{
    const key = this.graph_object_key(channel, object);
    var state = this.graph_object_states.get(key);

    if(!state)
    {
        state = this.graph_init_object_state(channel, object);
        this.graph_object_states.set(key, state);
    }

    object.state = state;
    return state;
};

NV20GeForce.prototype.graph_lookup_object_state = function(channel, handle)
{
    const object = this.fifo_ramht_lookup(channel.id, handle);

    if(!object)
    {
        return null;
    }

    return this.graph_get_object_state(channel, object);
};

NV20GeForce.prototype.graph_dma_object_from_instance = function(instance, handle, class_id)
{
    const offset = instance & (NV20_PRAMIN_SIZE - 1) & ~0xF;
    const flags = this.ramin_read32(offset);
    const limit = this.ramin_read32(offset + 4);
    const base = this.ramin_read32(offset + 8);

    return {
        handle: handle >>> 0,
        class_id: class_id & 0xFFFF,
        instance: offset >>> 4,
        offset: offset,
        flags: flags,
        limit: limit,
        base: base & 0xFFFFF000,
        adjust: flags >>> 20,
        linear: !!(flags & 0x00002000),
        physical: !!(flags & 0x00020000),
    };
};

NV20GeForce.prototype.graph_dma_object_from_handle = function(channel, handle)
{
    const object = this.fifo_ramht_lookup(channel.id, handle);

    if(!object || !nv20_is_dma_class(object.class_id))
    {
        return null;
    }

    return this.graph_dma_object_from_instance(object.instance, handle, object.class_id);
};

NV20GeForce.prototype.graph_bind_surface_handle = function(channel, state, handle)
{
    const object = this.fifo_ramht_lookup(channel.id, handle);

    if(!object)
    {
        state.surface_handle = handle >>> 0;
        return true;
    }

    const object_state = this.graph_get_object_state(channel, object);

    if(nv20_is_surface2d_class(object.class_id))
    {
        state.surface = object_state;
        state.swizzled_surface = null;
        state.image_upload = null;
    }
    else if(nv20_is_swizzled_surface_class(object.class_id))
    {
        state.swizzled_surface = object_state;
        state.surface = null;
        state.image_upload = null;
    }
    else if(object.class_id === NV20_CLASS_CLIP)
    {
        state.clip = object_state;
    }

    return true;
};

NV20GeForce.prototype.graph_bind_context_handle = function(channel, state, handle)
{
    const object = this.fifo_ramht_lookup(channel.id, handle);

    if(!object)
    {
        return true;
    }

    const object_state = this.graph_get_object_state(channel, object);
    const class_id = object.class_id & 0xFFFF;

    if(nv20_is_surface2d_class(class_id) ||
        nv20_is_swizzled_surface_class(class_id) ||
        class_id === NV20_CLASS_CLIP)
    {
        return this.graph_bind_surface_handle(channel, state, handle);
    }

    if(class_id === NV20_CLASS_ROP)
    {
        state.rop_context = object_state;
    }
    else if(nv20_is_pattern_class(class_id))
    {
        state.pattern = object_state;
    }
    else if(class_id === NV20_CLASS_CHROMA)
    {
        state.chroma = object_state;
    }
    else if(class_id === NV20_CLASS_BETA)
    {
        state.beta_context = object_state;
    }

    return true;
};

NV20GeForce.prototype.graph_submit_swizzled_surface = function(channel, state, method, data)
{
    switch(method)
    {
        case 0x0184:
            state.swz_dma = this.graph_dma_object_from_handle(channel, data);
            return true;
        case 0x0300:
        {
            const width_bits = data >>> 16 & 0xFF;
            const height_bits = data >>> 24 & 0xFF;
            state.swz_format = data & 0xFFFF;
            state.swz_width = width_bits < 16 ? 1 << width_bits : 0;
            state.swz_height = height_bits < 16 ? 1 << height_bits : 0;

            if(!state.swz_width)
            {
                state.swz_width = 1;
            }

            if(!state.swz_height)
            {
                state.swz_height = 1;
            }

            return true;
        }
        case 0x0304:
            state.swz_offset = data >>> 0;
            return true;
        default:
            return false;
    }
};

NV20GeForce.prototype.graph_write_swizzled_surface_pixel = function(surface, x, y, bytes)
{
    if(!surface || !surface.swz_width || !surface.swz_height)
    {
        return false;
    }

    x |= 0;
    y |= 0;

    if(x < 0 || y < 0 || x >= surface.swz_width || y >= surface.swz_height)
    {
        return false;
    }

    const bytes_per_pixel = nv20_render_bytes_per_pixel(
        nv20_swizzled_surface_bpp_from_format(surface.swz_format, this.render_bpp));
    const pixel = nv20_swizzle_index(x, y, surface.swz_width, surface.swz_height);
    const offset = surface.swz_offset + pixel * bytes_per_pixel >>> 0;

    for(var i = 0; i < bytes_per_pixel; i++)
    {
        this.graph_write_dma_byte(surface.swz_dma, offset + i >>> 0, bytes[i] || 0, true);
    }

    return true;
};

NV20GeForce.prototype.graph_effective_rop = function(state, operation)
{
    operation = operation === undefined ? state && state.operation : operation;

    if(operation !== undefined && operation !== 1)
    {
        return NV20_ROP_SRCCOPY;
    }

    if(state && state.rop_context && state.rop_context.rop !== undefined)
    {
        return state.rop_context.rop & 0xFF;
    }

    if(state && state.rop !== undefined)
    {
        return state.rop & 0xFF;
    }

    return NV20_ROP_SRCCOPY;
};

NV20GeForce.prototype.graph_count_2d = function(kind)
{
    const stats = this["graph_debug_2d"];

    if(!stats)
    {
        return;
    }

    stats[kind] = (stats[kind] || 0) + 1;
    stats["accel"] = this["graph_accel_count"] || 0;
};

NV20GeForce.prototype.graph_defer_vram_mark = function(bucket, offset, width)
{
    if(!bucket)
    {
        return;
    }

    offset >>>= 0;
    width >>>= 0;
    const end = Math.min(this.vram_size, offset + width);

    if(offset >= end)
    {
        return;
    }

    if(offset < bucket.dirty_min)
    {
        bucket.dirty_min = offset;
    }

    if(end > bucket.dirty_max)
    {
        bucket.dirty_max = end;
    }

    if(!this.render_frame_size || !this.render_stride)
    {
        return;
    }

    const render_start = this.render_offset;
    const render_end = render_start + this.render_frame_size;

    if(end <= render_start || offset >= render_end)
    {
        return;
    }

    const dirty_start = Math.max(offset, render_start);
    const dirty_end = Math.min(end, render_end);
    const dirty_min = dirty_start - render_start;
    const dirty_max = dirty_end - render_start;
    const min_y = Math.max(0, Math.min(this.render_height, dirty_min / this.render_stride | 0));
    const max_y = Math.max(min_y, Math.min(this.render_height,
        (dirty_max + this.render_stride - 1) / this.render_stride | 0));

    if(min_y >= max_y)
    {
        return;
    }

    if(!bucket.render_dirty_rows || bucket.render_dirty_rows.length !== this.render_height)
    {
        bucket.render_dirty_rows = new Uint8Array(this.render_height);
        bucket.render_dirty_row_min = this.render_height;
        bucket.render_dirty_row_max = 0;
    }

    for(var y = min_y; y < max_y; y++)
    {
        bucket.render_dirty_rows[y] = 1;
    }

    if(min_y < bucket.render_dirty_row_min)
    {
        bucket.render_dirty_row_min = min_y;
    }

    if(max_y > bucket.render_dirty_row_max)
    {
        bucket.render_dirty_row_max = max_y;
    }
};

NV20GeForce.prototype.graph_flush_vram_mark = function(bucket)
{
    if(!bucket || bucket.dirty_min >= bucket.dirty_max)
    {
        return;
    }

    if(bucket.dirty_min < this.vram_dirty_min)
    {
        this.vram_dirty_min = bucket.dirty_min;
    }

    if(bucket.dirty_max > this.vram_dirty_max)
    {
        this.vram_dirty_max = bucket.dirty_max;
    }

    if(bucket.render_dirty_rows &&
       bucket.render_dirty_row_min < bucket.render_dirty_row_max)
    {
        if(!this.render_dirty_rows || this.render_dirty_rows.length !== this.render_height)
        {
            this.render_dirty_rows = new Uint8Array(this.render_height);
        }

        const min_y = bucket.render_dirty_row_min;
        const max_y = bucket.render_dirty_row_max;

        for(var y = min_y; y < max_y; y++)
        {
            if(bucket.render_dirty_rows[y])
            {
                this.render_dirty_rows[y] = 1;
            }
        }

        if(min_y < this.render_dirty_row_min)
        {
            this.render_dirty_row_min = min_y;
        }

        if(max_y > this.render_dirty_row_max)
        {
            this.render_dirty_row_max = max_y;
        }

        const render_start = this.render_offset + min_y * this.render_stride;
        const render_end = this.render_offset + max_y * this.render_stride;

        if(render_start < this.render_dirty_min)
        {
            this.render_dirty_min = render_start;
        }

        if(render_end > this.render_dirty_max)
        {
            this.render_dirty_max = render_end;
        }

        this.activate_rendering();
        this.schedule_render();
    }
    else
    {
        this.vram_mark_write(bucket.dirty_min,
                             bucket.dirty_max - bucket.dirty_min,
                             0);
    }

    bucket.dirty_min = this.vram_size;
    bucket.dirty_max = 0;
    bucket.render_dirty_rows = null;
    bucket.render_dirty_row_min = this.render_height;
    bucket.render_dirty_row_max = 0;
};

NV20GeForce.prototype.graph_beta_value = function(state)
{
    if(state && state.beta_context && state.beta_context.beta !== undefined)
    {
        return state.beta_context.beta >>> 0;
    }

    if(state && state.beta !== undefined)
    {
        return state.beta >>> 0;
    }

    return 0xFFFFFFFF;
};

NV20GeForce.prototype.graph_chroma_matches = function(state, src_color, src_bytes)
{
    const chroma = state && state.chroma;

    if(!chroma || !chroma.chroma_color_set)
    {
        return false;
    }

    const chroma_color = chroma.chroma_color >>> 0;
    src_color >>>= 0;

    if(src_bytes >= 4)
    {
        return (src_color & 0x00FFFFFF) === (chroma_color & 0x00FFFFFF);
    }

    if(src_bytes === 2)
    {
        return (src_color & 0x0000FFFF) === (chroma_color & 0x0000FFFF);
    }

    return (src_color & 0x000000FF) === (chroma_color & 0x000000FF);
};

NV20GeForce.prototype.graph_blend_bytes = function(state,
                                                   src_color,
                                                   src_bpp,
                                                   src_format,
                                                   dst_color,
                                                   dst_bpp,
                                                   dst_format)
{
    src_color >>>= 0;
    dst_color >>>= 0;

    const beta = this.graph_beta_value(state);
    var src = nv20_pixel_channels(src_color, src_bpp, src_format);
    const dst = nv20_pixel_channels(dst_color, dst_bpp, dst_format);
    var r;
    var g;
    var b;

    if(src_bpp === 32)
    {
        // NV4 image blending treats a zero dword as transparent and otherwise
        // expects premultiplied source color.
        if(src_color === 0)
        {
            return null;
        }

        const src_format8 = src_format & 0xFF;
        const class_id = state && state.class_id || 0;
        const xrgb32 = nv20_is_sifm_class(class_id) ? src_format8 === 4 : src_format8 === 5;

        if(xrgb32)
        {
            src.a = 0xFF;
        }

        if(beta !== 0xFFFFFFFF)
        {
            src.b = src.b * (beta & 0xFF) / 0xFF | 0;
            src.g = src.g * (beta >> 8 & 0xFF) / 0xFF | 0;
            src.r = src.r * (beta >> 16 & 0xFF) / 0xFF | 0;
            src.a = src.a * (beta >>> 24) / 0xFF | 0;
        }

        if(src.a <= 0)
        {
            return null;
        }

        const inverse_alpha = 0xFF - src.a;
        r = nv20_clamp8(dst.r * inverse_alpha / 0xFF + src.r);
        g = nv20_clamp8(dst.g * inverse_alpha / 0xFF + src.g);
        b = nv20_clamp8(dst.b * inverse_alpha / 0xFF + src.b);
    }
    else
    {
        const inverse_beta = 0xFF - (beta >>> 24);
        r = nv20_clamp8((dst.r * inverse_beta + src.r * (beta >> 16 & 0xFF)) / 0xFF);
        g = nv20_clamp8((dst.g * inverse_beta + src.g * (beta >> 8 & 0xFF)) / 0xFF);
        b = nv20_clamp8((dst.b * inverse_beta + src.b * (beta & 0xFF)) / 0xFF);
    }

    return nv20_color_to_bytes(0xFF000000 | r << 16 | g << 8 | b,
                               32,
                               0,
                               dst_bpp,
                               dst_format);
};

NV20GeForce.prototype.graph_blend_fast_path = function(state, src_color, src_bpp, src_format)
{
    src_color >>>= 0;

    if(this.graph_beta_value(state) !== 0xFFFFFFFF)
    {
        return 0;
    }

    if(src_bpp !== 32)
    {
        return 1;
    }

    const src_format8 = src_format & 0xFF;
    const class_id = state && state.class_id || 0;
    const xrgb32 = nv20_is_sifm_class(class_id) ? src_format8 === 4 : src_format8 === 5;

    if(src_color === 0)
    {
        return -1;
    }

    if(xrgb32 || (src_color >>> 24) === 0xFF)
    {
        return 1;
    }

    if((src_color >>> 24) === 0)
    {
        return -1;
    }

    return 0;
};

NV20GeForce.prototype.graph_note_2d = function(kind, state, details)
{
    const stats = this["graph_debug_2d"];

    if(!stats)
    {
        return;
    }

    stats[kind] = (stats[kind] || 0) + 1;
    stats["accel"] = this["graph_accel_count"] || 0;

    const entry = details || {};
    entry["kind"] = kind;

    if(state)
    {
        entry["cls"] = state.class_name || nv20_class_name(state.class_id);
    }

    if(entry["ms"] !== undefined)
    {
        stats[kind + "_ms"] = (stats[kind + "_ms"] || 0) + entry["ms"];
    }

    stats["last"] = entry;
    stats["recent"].push(entry);

    if(stats["recent"].length > NV20_GRAPH_DEBUG_RECENT_LIMIT)
    {
        stats["recent"].shift();
    }
};

NV20GeForce.prototype.graph_note_method = function(state, method, data, handled)
{
    const stats = this["graph_debug_2d"];

    if(!stats || !state)
    {
        return;
    }

    const class_name = state.class_name || nv20_class_name(state.class_id);
    const data_method = method >= 0x0400 && method < 0x2000;

    if(handled && data_method)
    {
        stats["methods"]++;
        stats["data_methods"] = (stats["data_methods"] || 0) + 1;
        return;
    }

    const method_name = class_name + ":" + h(method >>> 0, 4);

    stats["methods"]++;
    stats["by_class"][class_name] = (stats["by_class"][class_name] || 0) + 1;
    stats["by_method"][method_name] = (stats["by_method"][method_name] || 0) + 1;

    if(!handled)
    {
        stats["recent"].push({
            "kind": "unhandled",
            "cls": class_name,
            "method": "0x" + h(method >>> 0, 4),
            "data": "0x" + h(data >>> 0, 8),
        });

        if(stats["recent"].length > NV20_GRAPH_DEBUG_RECENT_LIMIT)
        {
            stats["recent"].shift();
        }
    }
};

NV20GeForce.prototype.graph_decode_dma_address = function(dma, offset, prefer_vram)
{
    offset >>>= 0;

    var address = offset;

    if(dma)
    {
        const translated = this.dma_translate(dma, offset);

        if(!translated)
        {
            return null;
        }

        address = translated.address;

        if(!translated.physical)
        {
            return {
                kind: "vram",
                offset: address % this.vram_size,
            };
        }

        if(this.cpu && this.cpu.mem8 && address < this.cpu.mem8.length)
        {
            return {
                kind: "mem",
                offset: address >>> 0,
            };
        }

        return null;
    }

    const vram_base = this.vram_base >>> 0;
    const vram_limit = vram_base + this.vram_size >>> 0;

    if(address >= vram_base && address < vram_limit)
    {
        return {
            kind: "vram",
            offset: address - vram_base >>> 0,
        };
    }

    if(prefer_vram && address < this.vram_size)
    {
        return {
            kind: "vram",
            offset: address >>> 0,
        };
    }

    if(this.cpu && this.cpu.mem8 && address < this.cpu.mem8.length)
    {
        return {
            kind: "mem",
            offset: address >>> 0,
        };
    }

    if(address < this.vram_size)
    {
        return {
            kind: "vram",
            offset: address >>> 0,
        };
    }

    return null;
};

NV20GeForce.prototype.graph_read_dma_byte = function(dma, offset, prefer_vram)
{
    const location = this.graph_decode_dma_address(dma, offset, prefer_vram);

    if(!location)
    {
        return 0;
    }

    if(location.kind === "vram")
    {
        return this.vram[this.vram_offset(location.offset)];
    }

    return this.cpu.mem8[location.offset];
};

NV20GeForce.prototype.graph_write_dma_byte = function(dma, offset, value, prefer_vram)
{
    const location = this.graph_decode_dma_address(dma, offset, prefer_vram);

    if(!location)
    {
        return;
    }

    value &= 0xFF;

    if(location.kind === "vram")
    {
        const vram_offset = this.vram_offset(location.offset);
        this.vram[vram_offset] = value;
        this.vram_mark_write(vram_offset, 1, value);
    }
    else
    {
        this.cpu.mem8[location.offset] = value;
    }
};

NV20GeForce.prototype.graph_read_dma_pixel = function(dma, offset, bytes, prefer_vram)
{
    var value = 0;

    for(var i = 0; i < bytes; i++)
    {
        value |= this.graph_read_dma_byte(dma, offset + i >>> 0, prefer_vram) << (i << 3);
    }

    return value >>> 0;
};

NV20GeForce.prototype.graph_read_dma32 = function(dma, offset, prefer_vram)
{
    return this.graph_read_dma_pixel(dma, offset, 4, prefer_vram);
};

NV20GeForce.prototype.graph_dma_linear_location = function(dma, offset, prefer_vram)
{
    if(dma && !dma.linear)
    {
        return null;
    }

    return this.graph_decode_dma_address(dma, offset, prefer_vram);
};

NV20GeForce.prototype.graph_read_linear_pixel = function(location, byte_offset, bytes)
{
    var value = 0;

    if(!location)
    {
        return 0;
    }

    byte_offset >>>= 0;

    if(location.kind === "vram")
    {
        const base = location.offset + byte_offset >>> 0;

        for(var i = 0; i < bytes; i++)
        {
            value |= this.vram[this.vram_offset(base + i)] << (i << 3);
        }
    }
    else
    {
        const mem8 = this.cpu && this.cpu.mem8;
        const base = location.offset + byte_offset >>> 0;

        if(!mem8 || base >= mem8.length)
        {
            return 0;
        }

        const available = Math.min(bytes, mem8.length - base);

        for(var i = 0; i < available; i++)
        {
            value |= mem8[base + i] << (i << 3);
        }
    }

    return value >>> 0;
};

NV20GeForce.prototype.graph_write_dma32 = function(dma, offset, value, prefer_vram)
{
    value >>>= 0;

    for(var i = 0; i < 4; i++)
    {
        this.graph_write_dma_byte(dma, offset + i >>> 0, value >>> (i << 3), prefer_vram);
    }
};

NV20GeForce.prototype.graph_surface_bytes_per_pixel = function(surface)
{
    return nv20_render_bytes_per_pixel(nv20_surface_bpp_from_format(
        surface && surface.format || 0,
        this.render_bpp));
};

NV20GeForce.prototype.graph_surface_pitch = function(surface, destination)
{
    surface = surface || this.graph_default_surface();

    return (destination ? surface.dst_pitch : surface.src_pitch) ||
        this.render_stride ||
        this.render_width * this.graph_surface_bytes_per_pixel(surface);
};

NV20GeForce.prototype.graph_surface_offset = function(surface, destination)
{
    surface = surface || this.graph_default_surface();

    return ((destination ? surface.dst_offset : surface.src_offset) || 0) >>> 0;
};

NV20GeForce.prototype.graph_surface_dma = function(surface, destination)
{
    surface = surface || this.graph_default_surface();

    return destination ? surface.dst_dma : surface.src_dma;
};

NV20GeForce.prototype.graph_surface_location = function(surface, destination, byte_offset)
{
    surface = surface || this.graph_default_surface();

    return this.graph_decode_dma_address(
        this.graph_surface_dma(surface, destination),
        this.graph_surface_offset(surface, destination) + byte_offset >>> 0,
        true);
};

NV20GeForce.prototype.graph_clip_rect = function(surface, x, y, w, h, clip)
{
    x |= 0;
    y |= 0;
    w |= 0;
    h |= 0;

    if(w <= 0 || h <= 0)
    {
        return null;
    }

    if(clip && clip.clip_size && clip.clip_size.w && clip.clip_size.h)
    {
        const min_x = clip.clip_point.x | 0;
        const min_y = clip.clip_point.y | 0;
        const max_x = min_x + (clip.clip_size.w | 0);
        const max_y = min_y + (clip.clip_size.h | 0);

        if(x < min_x)
        {
            w -= min_x - x;
            x = min_x;
        }

        if(y < min_y)
        {
            h -= min_y - y;
            y = min_y;
        }

        if(x + w > max_x)
        {
            w = max_x - x;
        }

        if(y + h > max_y)
        {
            h = max_y - y;
        }
    }

    const bytes_per_pixel = this.graph_surface_bytes_per_pixel(surface);
    const pitch = this.graph_surface_pitch(surface, true);
    const surface_offset = this.graph_surface_offset(surface, true);
    const max_width = Math.max(this.render_width, pitch / bytes_per_pixel | 0);
    const max_height = Math.max(this.render_height, (this.vram_size - surface_offset) / Math.max(1, pitch) | 0);

    if(x < 0)
    {
        w += x;
        x = 0;
    }

    if(y < 0)
    {
        h += y;
        y = 0;
    }

    if(x + w > max_width)
    {
        w = max_width - x;
    }

    if(y + h > max_height)
    {
        h = max_height - y;
    }

    if(w <= 0 || h <= 0)
    {
        return null;
    }

    return {
        x: x,
        y: y,
        w: w,
        h: h,
    };
};

NV20GeForce.prototype.graph_write_surface_pixel = function(surface, x, y, bytes, rop, pattern_bytes, options)
{
    const bytes_per_pixel = this.graph_surface_bytes_per_pixel(surface);
    const pitch = this.graph_surface_pitch(surface, true);

    x |= 0;
    y |= 0;

    const surface_offset = this.graph_surface_offset(surface, true);
    const max_width = Math.max(this.render_width, pitch / bytes_per_pixel | 0);
    const max_height = Math.max(this.render_height, (this.vram_size - surface_offset) / Math.max(1, pitch) | 0);

    if(x < 0 || y < 0 || x >= max_width || y >= max_height)
    {
        return;
    }

    const location = this.graph_surface_location(surface, true, y * pitch + x * bytes_per_pixel >>> 0);

    if(!location)
    {
        return;
    }

    rop = rop === undefined ? NV20_ROP_SRCCOPY : rop & 0xFF;
    var write_bytes = bytes;

    if(options && options.operation === 5)
    {
        const fast_path = this.graph_blend_fast_path(options.state,
                                                     options.src_color,
                                                     options.src_bpp,
                                                     options.src_format);

        if(fast_path < 0)
        {
            this.graph_count_2d("blend_skip");
            return;
        }

        if(fast_path > 0)
        {
            rop = NV20_ROP_SRCCOPY;
            pattern_bytes = null;
            this.graph_count_2d("blend_copy");
        }
        else
        {
            var dst_color = 0;

            for(var k = 0; k < bytes_per_pixel; k++)
            {
                const dst_byte = location.kind === "vram" ?
                    this.vram[this.vram_offset(location.offset + k)] :
                    this.cpu.mem8[location.offset + k];
                dst_color |= dst_byte << (k << 3);
            }

            const dst_bpp = options.dst_bpp ||
                nv20_surface_bpp_from_format(surface.format, this.render_bpp);
            const blended = this.graph_blend_bytes(options.state,
                                                   options.src_color,
                                                   options.src_bpp,
                                                   options.src_format,
                                                   dst_color,
                                                   dst_bpp,
                                                   surface.format);

            if(!blended)
            {
                this.graph_count_2d("blend_skip");
                return;
            }

            write_bytes = blended;
            rop = NV20_ROP_SRCCOPY;
            pattern_bytes = null;
            this.graph_count_2d("blend");
        }
    }

    if(location.kind === "vram")
    {
        const offset = this.vram_offset(location.offset);

        for(var i = 0; i < bytes_per_pixel; i++)
        {
            const dst_offset = this.vram_offset(offset + i);
            const src = write_bytes[i] || 0;
            const dst = this.vram[dst_offset];
            const pat = pattern_bytes ? pattern_bytes[i] || 0 : src;
            this.vram[dst_offset] = nv20_rop_byte(rop, src, dst, pat);
        }

        if(options && options.defer_mark)
        {
            this.graph_defer_vram_mark(options.defer_mark, offset, bytes_per_pixel);
        }
        else
        {
            this.vram_mark_write(offset, bytes_per_pixel, write_bytes[0] || 0);
        }
    }
    else
    {
        for(var j = 0; j < bytes_per_pixel; j++)
        {
            const src = write_bytes[j] || 0;
            const dst = this.cpu.mem8[location.offset + j];
            const pat = pattern_bytes ? pattern_bytes[j] || 0 : src;
            this.cpu.mem8[location.offset + j] = nv20_rop_byte(rop, src, dst, pat);
        }
    }
};

NV20GeForce.prototype.graph_fill_rect = function(surface, x, y, w, h, color, color_format, clip, rop, state)
{
    surface = surface || this.graph_default_surface();
    this.update_render_mode_from_surface(surface, "2d-fill", x + w, y + h);

    const rect = this.graph_clip_rect(surface, x, y, w, h, clip);

    if(!rect)
    {
        return false;
    }

    const start_ms = nv20_now_ms();
    const bytes_per_pixel = this.graph_surface_bytes_per_pixel(surface);
    const pitch = this.graph_surface_pitch(surface, true);
    const dst_bpp = nv20_surface_bpp_from_format(surface.format, this.render_bpp);
    const src_bpp = nv20_solid_bpp_from_format(color_format, dst_bpp, state);
    const bytes = nv20_color_to_bytes(color, src_bpp, src_bpp === 16 ? color_format : 0,
                                      dst_bpp, surface.format);
    const row_bytes = rect.w * bytes_per_pixel;
    const operation = state && state.operation !== undefined ? state.operation >>> 0 : 0;
    rop = rop === undefined ? NV20_ROP_SRCCOPY : rop & 0xFF;

    if(rop !== NV20_ROP_SRCCOPY || operation === 5)
    {
        const dirty = {
            dirty_min: this.vram_size,
            dirty_max: 0,
        };

        for(var py = 0; py < rect.h; py++)
        {
            for(var px = 0; px < rect.w; px++)
            {
                this.graph_write_surface_pixel(surface, rect.x + px, rect.y + py, bytes, rop, null, {
                    operation: operation,
                    state: state,
                    src_color: color >>> 0,
                    src_bpp: src_bpp,
                    src_format: color_format >>> 0,
                    dst_bpp: dst_bpp,
                    defer_mark: dirty,
                });
            }
        }

        this.graph_flush_vram_mark(dirty);
        this["graph_accel_count"]++;
        this.graph_note_2d("fill", null, {
            x: rect.x,
            y: rect.y,
            w: rect.w,
            h: rect.h,
            rop: rop,
            op: operation,
            fmt: color_format >>> 0,
            ms: nv20_now_ms() - start_ms,
        });
        return true;
    }

    for(var yy = 0; yy < rect.h; yy++)
    {
        const location = this.graph_surface_location(surface, true,
            (rect.y + yy) * pitch + rect.x * bytes_per_pixel >>> 0);

        if(!location)
        {
            continue;
        }

        if(location.kind === "vram")
        {
            var offset = this.vram_offset(location.offset);
            const end = Math.min(this.vram_size, offset + row_bytes);

            if(bytes_per_pixel === 1)
            {
                this.vram.fill(bytes[0], offset, end);
            }
            else
            {
                for(var dst = offset; dst < end; dst += bytes_per_pixel)
                {
                    for(var b = 0; b < bytes_per_pixel && dst + b < end; b++)
                    {
                        this.vram[dst + b] = bytes[b] || 0;
                    }
                }
            }

            this.vram_mark_write(offset, end - offset, color);
        }
        else
        {
            for(var xx = 0; xx < rect.w; xx++)
            {
                for(var bb = 0; bb < bytes_per_pixel; bb++)
                {
                    this.cpu.mem8[location.offset + xx * bytes_per_pixel + bb] = bytes[bb] || 0;
                }
            }
        }
    }

    this["graph_accel_count"]++;
    this.graph_note_2d("fill", null, {
        x: rect.x,
        y: rect.y,
        w: rect.w,
        h: rect.h,
        rop: rop,
        op: operation,
        fmt: color_format >>> 0,
        ms: nv20_now_ms() - start_ms,
    });
    return true;
};

NV20GeForce.prototype.graph_blit = function(surface, src_x, src_y, dst_x, dst_y, w, h, clip, rop, state)
{
    surface = surface || this.graph_default_surface();
    this.update_render_mode_from_surface(surface, "2d-blit", dst_x + w, dst_y + h);

    const rect = this.graph_clip_rect(surface, dst_x, dst_y, w, h, clip);

    if(!rect)
    {
        return false;
    }

    const start_ms = nv20_now_ms();
    src_x += rect.x - dst_x;
    src_y += rect.y - dst_y;

    const bytes_per_pixel = this.graph_surface_bytes_per_pixel(surface);
    const src_pitch = this.graph_surface_pitch(surface, false);
    const dst_pitch = this.graph_surface_pitch(surface, true);
    const row_bytes = rect.w * bytes_per_pixel;
    const bottom_up = rect.y > src_y;
    const row = new Uint8Array(row_bytes);
    const use_chroma = !!(state && state.chroma && state.chroma.chroma_color_set);
    const dirty = {
        dirty_min: this.vram_size,
        dirty_max: 0,
    };
    rop = rop === undefined ? NV20_ROP_SRCCOPY : rop & 0xFF;

    for(var row_index = 0; row_index < rect.h; row_index++)
    {
        const yy = bottom_up ? rect.h - 1 - row_index : row_index;
        const src_location = this.graph_surface_location(surface, false,
            (src_y + yy) * src_pitch + src_x * bytes_per_pixel >>> 0);
        const dst_location = this.graph_surface_location(surface, true,
            (rect.y + yy) * dst_pitch + rect.x * bytes_per_pixel >>> 0);

        if(!src_location || !dst_location)
        {
            continue;
        }

        if(rop !== NV20_ROP_SRCCOPY || use_chroma)
        {
            for(var px = 0; px < rect.w; px++)
            {
                var src_color = 0;

                if(use_chroma)
                {
                    for(var cb = 0; cb < bytes_per_pixel; cb++)
                    {
                        const src_byte = src_location.kind === "vram" ?
                            this.vram[this.vram_offset(src_location.offset + px * bytes_per_pixel + cb)] :
                            this.cpu.mem8[src_location.offset + px * bytes_per_pixel + cb];
                        src_color |= src_byte << (cb << 3);
                    }

                    if(this.graph_chroma_matches(state, src_color, bytes_per_pixel))
                    {
                        this.graph_count_2d("chroma_skip");
                        continue;
                    }
                }

                for(var i = 0; i < bytes_per_pixel; i++)
                {
                    const byte_offset = px * bytes_per_pixel + i;
                    const src_value = src_location.kind === "vram" ?
                        this.vram[this.vram_offset(src_location.offset + byte_offset)] :
                        this.cpu.mem8[src_location.offset + byte_offset];
                    const dst_value = dst_location.kind === "vram" ?
                        this.vram[this.vram_offset(dst_location.offset + byte_offset)] :
                        this.cpu.mem8[dst_location.offset + byte_offset];
                    const value = nv20_rop_byte(rop, src_value, dst_value, src_value);

                    if(dst_location.kind === "vram")
                    {
                        const offset = this.vram_offset(dst_location.offset + byte_offset);
                        this.vram[offset] = value;
                        this.graph_defer_vram_mark(dirty, offset, 1);
                    }
                    else
                    {
                        this.cpu.mem8[dst_location.offset + byte_offset] = value;
                    }
                }
            }
        }
        else if(src_location.kind === "vram" && dst_location.kind === "vram")
        {
            const src_offset = this.vram_offset(src_location.offset);
            const dst_offset = this.vram_offset(dst_location.offset);
            row.fill(0);
            row.set(this.vram.subarray(src_offset, Math.min(this.vram_size, src_offset + row_bytes)));
            this.vram.set(row.subarray(0, Math.min(row_bytes, this.vram_size - dst_offset)), dst_offset);
            this.vram_mark_write(dst_offset, Math.min(row_bytes, this.vram_size - dst_offset), 0);
        }
        else
        {
            for(var i = 0; i < row_bytes; i++)
            {
                const value = src_location.kind === "vram" ?
                    this.vram[this.vram_offset(src_location.offset + i)] :
                    this.cpu.mem8[src_location.offset + i];

                if(dst_location.kind === "vram")
                {
                    const offset = this.vram_offset(dst_location.offset + i);
                    this.vram[offset] = value;
                    this.graph_defer_vram_mark(dirty, offset, 1);
                }
                else
                {
                    this.cpu.mem8[dst_location.offset + i] = value;
                }
            }
        }
    }

    this.graph_flush_vram_mark(dirty);
    this["graph_accel_count"]++;
    this.graph_note_2d("blit", null, {
        sx: src_x,
        sy: src_y,
        x: rect.x,
        y: rect.y,
        w: rect.w,
        h: rect.h,
        rop: rop,
        op: state && state.operation !== undefined ? state.operation >>> 0 : 0,
        ms: nv20_now_ms() - start_ms,
    });
    return true;
};

NV20GeForce.prototype.graph_begin_image_upload = function(state)
{
    const surface = state.surface || this.graph_default_surface();
    const is_sifc = nv20_is_sifc_class(state.class_id);
    const is_texupload = nv20_is_texupload_class(state.class_id);
    var point = state.point;
    var size = state.size_in.w && state.size_in.h ? state.size_in : state.size_out;
    var dest_w = state.size_out.w || state.size_in.w;
    var dest_h = state.size_out.h || state.size_in.h;

    if(is_sifc)
    {
        point = state.clip_size.w && state.clip_size.h ? state.clip_point : state.point12d4;
        size = state.size_in;
        dest_w = state.clip_size.w || state.size_in.w;
        dest_h = state.clip_size.h || state.size_in.h;
    }
    else if(is_texupload)
    {
        point = state.point;
        size = state.size;
        dest_w = state.size.w;
        dest_h = state.size.h;
    }

    if(!size.w || !size.h)
    {
        return null;
    }

    const dst_bpp = nv20_surface_bpp_from_format(surface.format, this.render_bpp);
    const allow_mono = nv20_is_ifc_class(state.class_id) ?
        state.ifc_indexed_format :
        (is_sifc && (state.color_format & 0xFF) === 0);
    const src_bpp = state.color_format_set ?
        nv20_ifc_bpp_from_format(state.color_format, dst_bpp, surface.format, allow_mono) :
        dst_bpp;
    const src_bytes = src_bpp === 1 ? 0 : nv20_render_bytes_per_pixel(src_bpp);
    const mono_words_per_row = src_bpp === 1 ? Math.max(1, size.w + 31 >>> 5) : 0;
    const clip_enabled = !!(is_texupload && state.tex_clip_size.w && state.tex_clip_size.h);
    this.update_render_mode_from_surface(surface, "2d-upload",
                                         point.x + dest_w, point.y + dest_h);

    state.image_upload = {
        surface: surface,
        x: point.x | 0,
        y: point.y | 0,
        width: size.w | 0,
        height: size.h | 0,
        dest_width: dest_w | 0,
        dest_height: dest_h | 0,
        src_bpp: src_bpp,
        src_bytes: src_bytes,
        dst_bpp: dst_bpp,
        bytes_total: src_bpp === 1 ?
            mono_words_per_row * 4 * size.h :
            size.w * size.h * src_bytes,
        bytes_done: 0,
        pixel_byte: 0,
        pixel_value: 0,
        pixel_index: 0,
        mono_words_per_row: mono_words_per_row,
        mono_lsb_first: !(state.ifc_index_format & 1),
        rop: this.graph_effective_rop(state, state.operation),
        operation: state.operation >>> 0,
        src_format: state.color_format >>> 0,
        clip_enabled: clip_enabled,
        clip_x0: state.tex_clip_point.x | 0,
        clip_y0: state.tex_clip_point.y | 0,
        clip_x1: (state.tex_clip_point.x | 0) + (state.tex_clip_size.w | 0),
        clip_y1: (state.tex_clip_point.y | 0) + (state.tex_clip_size.h | 0),
        start_ms: nv20_now_ms(),
        dirty_min: this.vram_size,
        dirty_max: 0,
    };

    if(src_bpp === 1)
    {
        this["graph_mono_upload_count"]++;
    }

    return state.image_upload;
};

NV20GeForce.prototype.graph_upload_mono_word = function(state, data, upload)
{
    const surface = upload.surface;
    const color0 = nv20_color_to_bytes(state.mono_color0,
                                       upload.dst_bpp,
                                       surface.format,
                                       upload.dst_bpp,
                                       surface.format);
    const color1 = nv20_color_to_bytes(state.mono_color1,
                                       upload.dst_bpp,
                                       surface.format,
                                       upload.dst_bpp,
                                       surface.format);
    const word_index = upload.bytes_done >>> 2;
    const row = word_index / upload.mono_words_per_row | 0;
    const word_in_row = word_index - row * upload.mono_words_per_row;

    if(row >= upload.height)
    {
        upload.bytes_done = upload.bytes_total;
        return true;
    }

    for(var bit_index = 0; bit_index < 32; bit_index++)
    {
        const src_x = word_in_row * 32 + bit_index;

        if(src_x >= upload.width)
        {
            continue;
        }

        const bit = upload.mono_lsb_first ? bit_index : 31 - bit_index;
        const dst_x = upload.x + (src_x * upload.dest_width / upload.width | 0);
        const dst_y = upload.y + (row * upload.dest_height / upload.height | 0);
        const bytes = data >>> bit & 1 ? color1 : color0;

        if(upload.clip_enabled &&
            (dst_x < upload.clip_x0 || dst_x >= upload.clip_x1 ||
             dst_y < upload.clip_y0 || dst_y >= upload.clip_y1))
        {
            continue;
        }

        this.graph_write_surface_pixel(surface, dst_x, dst_y, bytes, upload.rop, null, {
            defer_mark: upload,
        });
    }

    upload.bytes_done += 4;

    if(upload.bytes_done >= upload.bytes_total)
    {
        this.graph_flush_vram_mark(upload);
        this.graph_note_2d("upload", state, {
            x: upload.x,
            y: upload.y,
            w: upload.dest_width,
            h: upload.dest_height,
            src_w: upload.width,
            src_h: upload.height,
            src_bpp: upload.src_bpp,
            dst_bpp: upload.dst_bpp,
            fmt: upload.src_format >>> 0,
            op: upload.operation >>> 0,
            rop: upload.rop,
            mono: true,
            ms: nv20_now_ms() - upload.start_ms,
        });
    }

    this.graph_count_2d("mono_words");
    return true;
};

NV20GeForce.prototype.graph_upload_data_word = function(state, data)
{
    var upload = state.image_upload;

    if(!upload || upload.bytes_done >= upload.bytes_total)
    {
        upload = this.graph_begin_image_upload(state);
    }

    if(!upload)
    {
        return false;
    }

    if(upload.src_bpp === 1)
    {
        return this.graph_upload_mono_word(state, data, upload);
    }

    this.graph_count_2d("upload_words");

    for(var i = 0; i < 4 && upload.bytes_done < upload.bytes_total; i++)
    {
        const byte = data >>> (i << 3) & 0xFF;
        upload.pixel_value |= byte << (upload.pixel_byte << 3);
        upload.pixel_byte++;
        upload.bytes_done++;

        if(upload.pixel_byte === upload.src_bytes)
        {
            const src_x = upload.pixel_index % upload.width;
            const src_y = upload.pixel_index / upload.width | 0;
            const dst_x = upload.x + (src_x * upload.dest_width / upload.width | 0);
            const dst_y = upload.y + (src_y * upload.dest_height / upload.height | 0);
            const src_color = upload.pixel_value >>> 0;

            if(upload.clip_enabled &&
                (dst_x < upload.clip_x0 || dst_x >= upload.clip_x1 ||
                 dst_y < upload.clip_y0 || dst_y >= upload.clip_y1))
            {
                upload.pixel_value = 0;
                upload.pixel_byte = 0;
                upload.pixel_index++;
                continue;
            }

            if(this.graph_chroma_matches(state, src_color, upload.src_bytes))
            {
                this.graph_count_2d("chroma_skip");
                upload.pixel_value = 0;
                upload.pixel_byte = 0;
                upload.pixel_index++;
                continue;
            }

            const bytes = nv20_color_to_bytes(src_color,
                                              upload.src_bpp,
                                              state.color_format,
                                              upload.dst_bpp,
                                              upload.surface.format);

            this.graph_write_surface_pixel(upload.surface, dst_x, dst_y, bytes, upload.rop, null, {
                operation: upload.operation,
                state: state,
                src_color: src_color,
                src_bpp: upload.src_bpp,
                src_format: upload.src_format,
                dst_bpp: upload.dst_bpp,
                defer_mark: upload,
            });
            upload.pixel_value = 0;
            upload.pixel_byte = 0;
            upload.pixel_index++;
        }
    }

    if(upload.bytes_done >= upload.bytes_total)
    {
        this.graph_flush_vram_mark(upload);
        this.graph_note_2d("upload", state, {
            x: upload.x,
            y: upload.y,
            w: upload.dest_width,
            h: upload.dest_height,
            src_w: upload.width,
            src_h: upload.height,
            src_bpp: upload.src_bpp,
            dst_bpp: upload.dst_bpp,
            fmt: upload.src_format >>> 0,
            op: upload.operation >>> 0,
            rop: upload.rop,
            mono: false,
            ms: nv20_now_ms() - upload.start_ms,
        });
    }

    return true;
};

NV20GeForce.prototype.graph_begin_iifc_upload = function(state)
{
    const width = state.iifc_shw.w >>> 0;
    const height = state.iifc_shw.h >>> 0;
    const bits_per_symbol = state.iifc_bpp4 ? 4 : 8;
    const word_count = width && height ?
        (width * height * bits_per_symbol + 31 >>> 5) :
        0;

    state.iifc_words = word_count ? new Uint32Array(word_count) : null;
    state.iifc_words_ptr = 0;
    state.iifc_words_left = word_count;
};

NV20GeForce.prototype.graph_execute_iifc = function(state)
{
    const surface = state.surface || this.graph_default_surface();
    const dx = state.iifc_yx.x | 0;
    const dy = state.iifc_yx.y | 0;
    const swidth = state.iifc_shw.w >>> 0;
    const dwidth = state.iifc_dhw.w >>> 0;
    const height = state.iifc_dhw.h >>> 0;
    const words = state.iifc_words;

    if(!words || !swidth || !dwidth || !height)
    {
        return false;
    }

    const start_ms = nv20_now_ms();
    const src_bpp = nv20_iifc_bpp_from_format(state.iifc_color_format);
    const src_bytes = nv20_render_bytes_per_pixel(src_bpp);
    const dst_bpp = nv20_surface_bpp_from_format(surface.format, this.render_bpp);
    const operation = state.iifc_operation >>> 0;
    const rop = this.graph_effective_rop(state, operation);
    const clip = state.clip;
    const use_clip = !!(clip && clip.clip_size && clip.clip_size.w && clip.clip_size.h);
    const clip_x0 = use_clip ? clip.clip_point.x | 0 : 0;
    const clip_y0 = use_clip ? clip.clip_point.y | 0 : 0;
    const clip_x1 = use_clip ? clip_x0 + (clip.clip_size.w | 0) : 0;
    const clip_y1 = use_clip ? clip_y0 + (clip.clip_size.h | 0) : 0;
    const dirty = {
        dirty_min: this.vram_size,
        dirty_max: 0,
    };
    var symbol_index = 0;

    this.update_render_mode_from_surface(surface, "2d-iifc", dx + dwidth, dy + height);

    for(var y = 0; y < height; y++)
    {
        for(var x = 0; x < dwidth; x++)
        {
            const dst_x = dx + x;
            const dst_y = dy + y;

            if(!use_clip ||
                dst_x >= clip_x0 && dst_x < clip_x1 &&
                dst_y >= clip_y0 && dst_y < clip_y1)
            {
                var symbol;

                if(state.iifc_bpp4)
                {
                    const word_offset = symbol_index >>> 3;
                    const symbol_offset = (symbol_index & 7 ^ 1) << 2;
                    symbol = word_offset < words.length ?
                        words[word_offset] >>> symbol_offset & 0xF :
                        0;
                }
                else
                {
                    const word_offset = symbol_index >>> 2;
                    const symbol_offset = (symbol_index & 3) << 3;
                    symbol = word_offset < words.length ?
                        words[word_offset] >>> symbol_offset & 0xFF :
                        0;
                }

                const src_color = this.graph_read_dma_pixel(
                    state.iifc_palette_dma,
                    state.iifc_palette_offset + symbol * src_bytes >>> 0,
                    src_bytes,
                    false);
                const bytes = nv20_color_to_bytes(src_color,
                                                  src_bpp,
                                                  state.iifc_color_format,
                                                  dst_bpp,
                                                  surface.format);

                this.graph_write_surface_pixel(surface, dst_x, dst_y, bytes, rop, null, {
                    operation: operation,
                    state: state,
                    src_color: src_color,
                    src_bpp: src_bpp,
                    src_format: state.iifc_color_format,
                    dst_bpp: dst_bpp,
                    defer_mark: dirty,
                });
            }

            symbol_index++;
        }

        symbol_index += Math.max(0, swidth - dwidth);
    }

    this.graph_flush_vram_mark(dirty);
    this["graph_accel_count"]++;
    this.graph_note_2d("iifc", state, {
        x: dx,
        y: dy,
        w: dwidth,
        h: height,
        sw: swidth,
        bpp4: !!state.iifc_bpp4,
        fmt: state.iifc_color_format >>> 0,
        op: operation,
        rop: rop,
        src_bpp: src_bpp,
        dst_bpp: dst_bpp,
        ms: nv20_now_ms() - start_ms,
    });

    return true;
};

NV20GeForce.prototype.graph_begin_swizzled_upload = function(state)
{
    const surface = state.swizzled_surface;
    const width = state.size.w >>> 0;
    const height = state.size.h >>> 0;

    if(!surface || !width || !height)
    {
        return null;
    }

    const dst_bpp = nv20_swizzled_surface_bpp_from_format(surface.swz_format, this.render_bpp);
    const src_bpp = state.color_format_set ?
        nv20_ifc_bpp_from_format(state.color_format, dst_bpp, surface.swz_format, false) :
        dst_bpp;
    const src_bytes = nv20_render_bytes_per_pixel(src_bpp);

    if(!src_bytes)
    {
        return null;
    }

    const clip_enabled = !!(state.tex_clip_size.w && state.tex_clip_size.h);

    state.image_upload = {
        swizzled_surface: surface,
        x: state.point.x | 0,
        y: state.point.y | 0,
        width: width,
        height: height,
        src_bpp: src_bpp,
        src_bytes: src_bytes,
        dst_bpp: dst_bpp,
        bytes_total: width * height * src_bytes,
        bytes_done: 0,
        pixel_byte: 0,
        pixel_value: 0,
        pixel_index: 0,
        src_format: state.color_format >>> 0,
        clip_enabled: clip_enabled,
        clip_x0: state.tex_clip_point.x | 0,
        clip_y0: state.tex_clip_point.y | 0,
        clip_x1: (state.tex_clip_point.x | 0) + (state.tex_clip_size.w | 0),
        clip_y1: (state.tex_clip_point.y | 0) + (state.tex_clip_size.h | 0),
        start_ms: nv20_now_ms(),
    };

    return state.image_upload;
};

NV20GeForce.prototype.graph_upload_swizzled_data_word = function(state, data)
{
    var upload = state.image_upload;

    if(!upload ||
        !upload.swizzled_surface ||
        upload.swizzled_surface !== state.swizzled_surface ||
        upload.bytes_done >= upload.bytes_total)
    {
        upload = this.graph_begin_swizzled_upload(state);
    }

    if(!upload)
    {
        return false;
    }

    this.graph_count_2d("upload_words");

    for(var i = 0; i < 4 && upload.bytes_done < upload.bytes_total; i++)
    {
        const byte = data >>> (i << 3) & 0xFF;
        upload.pixel_value |= byte << (upload.pixel_byte << 3);
        upload.pixel_byte++;
        upload.bytes_done++;

        if(upload.pixel_byte === upload.src_bytes)
        {
            const src_x = upload.pixel_index % upload.width;
            const src_y = upload.pixel_index / upload.width | 0;
            const dst_x = upload.x + src_x;
            const dst_y = upload.y + src_y;

            if(!upload.clip_enabled ||
                dst_x >= upload.clip_x0 && dst_x < upload.clip_x1 &&
                dst_y >= upload.clip_y0 && dst_y < upload.clip_y1)
            {
                const src_color = upload.pixel_value >>> 0;
                const bytes = nv20_color_to_bytes(src_color,
                                                  upload.src_bpp,
                                                  upload.src_format,
                                                  upload.dst_bpp,
                                                  upload.swizzled_surface.swz_format);
                this.graph_write_swizzled_surface_pixel(upload.swizzled_surface,
                                                        dst_x,
                                                        dst_y,
                                                        bytes);
            }

            upload.pixel_value = 0;
            upload.pixel_byte = 0;
            upload.pixel_index++;
        }
    }

    if(upload.bytes_done >= upload.bytes_total)
    {
        this["graph_accel_count"]++;
        this.graph_note_2d("tex_swizzle", state, {
            x: upload.x,
            y: upload.y,
            w: upload.width,
            h: upload.height,
            src_bpp: upload.src_bpp,
            dst_bpp: upload.dst_bpp,
            fmt: upload.src_format >>> 0,
            ms: nv20_now_ms() - upload.start_ms,
        });
    }

    return true;
};

NV20GeForce.prototype.graph_m2mf_changed = function(state)
{
    state.m2mf_executed = false;
};

NV20GeForce.prototype.graph_execute_m2mf = function(state)
{
    if(state.m2mf_executed ||
        !state.m2mf_line_length ||
        !state.m2mf_line_count)
    {
        return false;
    }

    const line_length = state.m2mf_line_length >>> 0;
    const line_count = state.m2mf_line_count >>> 0;
    const pitch_in = state.m2mf_pitch_in || line_length;
    const pitch_out = state.m2mf_pitch_out || line_length;
    const row = new Uint8Array(line_length);
    const dirty = {
        dirty_min: this.vram_size,
        dirty_max: 0,
    };
    const start_ms = nv20_now_ms();
    var copied = 0;

    for(var y = 0; y < line_count; y++)
    {
        const src_offset = state.m2mf_offset_in + y * pitch_in >>> 0;
        const dst_offset = state.m2mf_offset_out + y * pitch_out >>> 0;
        const src_location = this.graph_decode_dma_address(state.m2mf_in_dma, src_offset, !state.m2mf_in_dma);
        const dst_location = this.graph_decode_dma_address(state.m2mf_out_dma, dst_offset, true);

        if(!src_location || !dst_location)
        {
            continue;
        }

        row.fill(0);

        if(src_location.kind === "vram")
        {
            const src = this.vram_offset(src_location.offset);

            if(src + line_length <= this.vram_size)
            {
                row.set(this.vram.subarray(src, src + line_length));
            }
            else
            {
                for(var sx = 0; sx < line_length; sx++)
                {
                    row[sx] = this.vram[this.vram_offset(src + sx)];
                }
            }
        }
        else
        {
            const src = src_location.offset >>> 0;
            const src_end = Math.min(this.cpu.mem8.length, src + line_length);

            if(src < src_end)
            {
                row.set(this.cpu.mem8.subarray(src, src_end));
            }
        }

        if(dst_location.kind === "vram")
        {
            const dst = this.vram_offset(dst_location.offset);

            if(dst + line_length <= this.vram_size)
            {
                this.vram.set(row, dst);
                this.graph_defer_vram_mark(dirty, dst, line_length);
            }
            else
            {
                for(var dx = 0; dx < line_length; dx++)
                {
                    const offset = this.vram_offset(dst + dx);
                    this.vram[offset] = row[dx];
                    this.graph_defer_vram_mark(dirty, offset, 1);
                }
            }
        }
        else
        {
            const dst = dst_location.offset >>> 0;
            const dst_end = Math.min(this.cpu.mem8.length, dst + line_length);

            if(dst < dst_end)
            {
                this.cpu.mem8.set(row.subarray(0, dst_end - dst), dst);
            }
        }

        copied += line_length;
    }

    this.graph_flush_vram_mark(dirty);
    state.m2mf_executed = true;
    this["graph_accel_count"]++;
    this.graph_note_2d("m2mf", state, {
        in: state.m2mf_offset_in >>> 0,
        out: state.m2mf_offset_out >>> 0,
        len: line_length,
        lines: line_count,
        pitch_in: pitch_in,
        pitch_out: pitch_out,
        bytes: copied,
        ms: nv20_now_ms() - start_ms,
    });
    return true;
};

NV20GeForce.prototype.graph_write_notify = function(state, base_offset)
{
    if(!state.notifier_dma)
    {
        return;
    }

    const timestamp = this.timer_read64();

    this.graph_write_dma32(state.notifier_dma, base_offset, timestamp >>> 0, false);
    this.graph_write_dma32(state.notifier_dma, base_offset + 4 >>> 0,
                           Math.floor(timestamp / 0x100000000) >>> 0, false);
    this.graph_write_dma32(state.notifier_dma, base_offset + 8 >>> 0, 0, false);
    this.graph_write_dma32(state.notifier_dma, base_offset + 12 >>> 0, 0, false);
};

NV20GeForce.prototype.graph_complete_notify = function(channel, subchannel, state, method, data)
{
    if(!state.notify_pending)
    {
        return;
    }

    state.notify_pending = false;
    this.graph_write_notify(state, 0);

    if(!state.notify_type)
    {
        return;
    }

    const notifier = state.notifier_dma && state.notifier_dma.instance || 0;

    this.graph_intr |= 0x00000001;
    this.graph_nsource |= 0x00000001;
    this.graph_notify = 0x00110000;
    this.graph_notify_instance = state.instance >>> 4;
    this.graph_ctx_switch2 = (notifier & 0xFFFF) << 16;
    this.graph_ctx_switch4 = state.instance >>> 4;
    this.graph_trapped_addr = method | subchannel << 16 | channel.id << 20;
    this.graph_trapped_data = data >>> 0;
    this.fifo_wait_notify = true;
    this.fifo_note_wait("notify");
    this.update_irq_level();
};

NV20GeForce.prototype.graph_submit_surface2d = function(channel, state, method, data)
{
    switch(method)
    {
        case 0x0184:
            state.src_dma = this.graph_dma_object_from_handle(channel, data);
            return true;
        case 0x0188:
            state.dst_dma = this.graph_dma_object_from_handle(channel, data);
            return true;
        case 0x0300:
            state.format = data & 0xFF;
            state.surface_format_set = true;
            this.update_render_mode_from_surface(state, "surface2d-format", 0, 0);
            return true;
        case 0x0304:
            state.src_pitch = data & 0xFFFF;
            state.dst_pitch = data >>> 16;
            state.surface_pitch_set = true;
            this.update_render_mode_from_surface(state, "surface2d-pitch", 0, 0);
            return true;
        case 0x0308:
            state.src_offset = data >>> 0;
            state.surface_src_offset_set = true;
            return true;
        case 0x030C:
            state.dst_offset = data >>> 0;
            state.surface_dst_offset_set = true;
            this.update_render_mode_from_surface(state, "surface2d-dst-offset", 0, 0);
            return true;
        default:
            return false;
    }
};

NV20GeForce.prototype.graph_submit_rect = function(channel, state, method, data)
{
    switch(method)
    {
        case 0x0184:
        case 0x0188:
        case 0x018C:
        case 0x0190:
            return this.graph_bind_context_handle(channel, state, data);
        case 0x0194:
        case 0x0198:
            return this.graph_bind_surface_handle(channel, state, data);
        case 0x02FC:
            state.operation = data;
            return true;
        case 0x0300:
            state.color_format = data;
            state.color_format_set = true;
            return true;
        case 0x0304:
            state.color = data;
            return true;
        default:
            if(method >= 0x0400 && method < 0x0480)
            {
                if((method & 4) === 0)
                {
                    state.rect_xy = nv20_unpack_xy(data);
                }
                else
                {
                    const size = nv20_unpack_wh(data);
                    this.graph_fill_rect(state.surface, state.rect_xy.x, state.rect_xy.y,
                                         size.w, size.h, state.color, state.color_format, state.clip,
                                         this.graph_effective_rop(state, state.operation),
                                         state);
                }

                return true;
            }

            return false;
    }
};

NV20GeForce.prototype.graph_gdi_color_bytes = function(state, surface, color)
{
    const dst_bpp = nv20_surface_bpp_from_format(surface.format, this.render_bpp);
    var src_bpp = dst_bpp;
    var src_format = state.gdi_color_format || state.color_format || 0;

    // NV4_GDI treats format 3 as native 32-bit color. Other GDI text colors
    // are 16-bit source colors even when the destination surface is 32-bit.
    if(dst_bpp === 32 && src_format !== 3)
    {
        src_bpp = 16;
        src_format = 1;
    }
    else if(dst_bpp === 16)
    {
        src_bpp = 16;
        src_format = surface.format || src_format;
    }

    return nv20_color_to_bytes(color, src_bpp, src_format, dst_bpp, surface.format);
};

NV20GeForce.prototype.graph_gdi_prepare_words = function(state, type)
{
    const width = state.gdi_image_swh.w >>> 0;
    const height = state.gdi_image_swh.h >>> 0;

    if(!width || !height || width > NV20_MAX_RENDER_WIDTH || height > NV20_MAX_RENDER_HEIGHT)
    {
        state.gdi_words = [];
        state.gdi_words_ptr = 0;
        state.gdi_words_left = 0;
        return false;
    }

    const word_count = Math.min(0x100000, Math.max(1, width * height + 31 >>> 5));

    state.gdi_words = new Array(word_count);
    state.gdi_words_ptr = 0;
    state.gdi_words_left = word_count;
    state.gdi_image_type = type >>> 0;
    return true;
};

NV20GeForce.prototype.graph_execute_gdi_blit = function(state, type)
{
    const surface = state.surface || this.graph_default_surface();
    const dx = state.gdi_image_xy.x | 0;
    const dy = state.gdi_image_xy.y | 0;
    const swidth = state.gdi_image_swh.w >>> 0;
    const height = state.gdi_image_swh.h >>> 0;
    const dwidth = type ? (state.gdi_image_dwh.w || swidth) >>> 0 : swidth;

    if(!swidth || !dwidth || !height)
    {
        return false;
    }

    const start_ms = nv20_now_ms();
    this.update_render_mode_from_surface(surface, "2d-gdi-image",
                                         dx + dwidth, dy + height);

    const clipx0 = (state.clip_xy0.x | 0) - dx;
    const clipy0 = (state.clip_xy0.y | 0) - dy;
    const clipx1 = (state.clip_xy1.x | 0) - dx;
    const clipy1 = (state.clip_xy1.y | 0) - dy;
    const bg_bytes = this.graph_gdi_color_bytes(state, surface, state.gdi_bg_color);
    const fg_bytes = this.graph_gdi_color_bytes(state, surface, state.gdi_fg_color);
    const rop = this.graph_effective_rop(state, state.operation);
    const dst_bpp = nv20_surface_bpp_from_format(surface.format, this.render_bpp);
    var src_bpp = dst_bpp;
    var src_format = state.gdi_color_format || state.color_format || 0;
    const dirty = {
        dirty_min: this.vram_size,
        dirty_max: 0,
    };

    if(dst_bpp === 32 && src_format !== 3)
    {
        src_bpp = 16;
        src_format = 1;
    }
    else if(dst_bpp === 16)
    {
        src_bpp = 16;
        src_format = surface.format || src_format;
    }

    var bit_index = 0;
    var wrote = false;

    for(var y = 0; y < height; y++)
    {
        for(var x = 0; x < dwidth; x++)
        {
            const word_offset = bit_index >>> 5;
            var bit_offset = bit_index & 31;

            if(state.gdi_mono_format === 1)
            {
                bit_offset ^= 7;
            }

            const pixel = !!((state.gdi_words[word_offset] || 0) >>> bit_offset & 1);

            if(x >= clipx0 && x < clipx1 && y >= clipy0 && y < clipy1 &&
                (type || pixel))
            {
                const src_color = pixel ? state.gdi_fg_color >>> 0 : state.gdi_bg_color >>> 0;

                this.graph_write_surface_pixel(surface, dx + x, dy + y,
                    pixel ? fg_bytes : bg_bytes, rop, null, {
                        operation: state.operation >>> 0,
                        state: state,
                        src_color: src_color,
                        src_bpp: src_bpp,
                        src_format: src_format,
                        dst_bpp: dst_bpp,
                        defer_mark: dirty,
                    });
                wrote = true;
            }

            bit_index++;
        }

        bit_index += swidth - dwidth;
    }

    state.gdi_words = [];
    state.gdi_words_ptr = 0;
    state.gdi_words_left = 0;

    if(wrote)
    {
        this.graph_flush_vram_mark(dirty);
        this["graph_accel_count"]++;
    }

    this.graph_note_2d("gdi_image", state, {
        x: dx,
        y: dy,
        w: dwidth,
        h: height,
        sw: swidth,
        type: type,
        rop: rop,
        op: state.operation >>> 0,
        src_bpp: src_bpp,
        src_fmt: src_format >>> 0,
        dst_bpp: dst_bpp,
        ms: nv20_now_ms() - start_ms,
    });
    return true;
};

NV20GeForce.prototype.graph_gdi_image_word = function(state, data, type)
{
    if(!state.gdi_words_left)
    {
        return true;
    }

    state.gdi_words[state.gdi_words_ptr++] = data >>> 0;
    state.gdi_words_left--;

    if(!state.gdi_words_left)
    {
        this.graph_execute_gdi_blit(state, type);
    }

    return true;
};

NV20GeForce.prototype.graph_submit_gdi = function(channel, state, method, data)
{
    const class_id = state.class_id & 0xFFFF;
    const is_nv4_gdi = class_id === NV20_CLASS_GDI_NV4;
    const is_nv3_gdi = class_id === NV20_CLASS_GDI_NV3;

    switch(method)
    {
        case 0x0198:
            return this.graph_bind_surface_handle(channel, state, data);
        case 0x0184:
        case 0x0188:
        case 0x018C:
        case 0x0190:
        case 0x0194:
            return this.graph_bind_context_handle(channel, state, data);
        case 0x02FC:
            state.operation = data;
            return true;
        case 0x0300:
            state.color_format = data;
            state.gdi_color_format = data >>> 0;
            state.color_format_set = true;
            return true;
        case 0x0304:
            state.gdi_mono_format = data >>> 0;
            return true;
        case 0x03FC:
            state.color = data;
            state.mono_color1 = data >>> 0;
            state.gdi_fg_color = data >>> 0;
            return true;
        case 0x05F4:
            state.clip_xy0 = nv20_unpack_xy(data);
            return true;
        case 0x05F8:
            state.clip_xy1 = nv20_unpack_xy(data);
            return true;
        case 0x05FC:
            state.color = data;
            state.mono_color1 = data >>> 0;
            state.gdi_fg_color = data >>> 0;
            return true;
        case 0x07EC:
            if(!is_nv4_gdi)
            {
                return false;
            }
            state.clip_xy0 = nv20_unpack_xy(data);
            return true;
        case 0x07F0:
            if(!is_nv4_gdi)
            {
                return false;
            }
            state.clip_xy1 = nv20_unpack_xy(data);
            return true;
        case 0x07F4:
            if(!is_nv4_gdi)
            {
                return false;
            }
            state.gdi_fg_color = data >>> 0;
            return true;
        case 0x07F8:
            if(!is_nv4_gdi)
            {
                return false;
            }
            state.gdi_image_swh = nv20_unpack_wh(data);
            return true;
        case 0x07FC:
            if(!is_nv4_gdi)
            {
                return false;
            }
            state.gdi_image_xy = nv20_unpack_xy(data);
            return this.graph_gdi_prepare_words(state, 0);
        case 0x0BE4:
            if(!is_nv4_gdi)
            {
                return false;
            }
            state.clip_xy0 = nv20_unpack_xy(data);
            return true;
        case 0x0BE8:
            if(!is_nv4_gdi)
            {
                return false;
            }
            state.clip_xy1 = nv20_unpack_xy(data);
            return true;
        case 0x0BEC:
            if(is_nv3_gdi)
            {
                state.clip_xy0 = nv20_unpack_xy(data);
            }
            else
            {
                state.gdi_bg_color = data >>> 0;
            }
            return true;
        case 0x0BF0:
            if(is_nv3_gdi)
            {
                state.clip_xy1 = nv20_unpack_xy(data);
            }
            else
            {
                state.gdi_fg_color = data >>> 0;
            }
            return true;
        case 0x0BF4:
            if(is_nv3_gdi)
            {
                state.gdi_fg_color = data >>> 0;
            }
            else
            {
                state.gdi_image_swh = nv20_unpack_wh(data);
            }
            return true;
        case 0x0BF8:
            if(is_nv3_gdi)
            {
                state.gdi_image_swh = nv20_unpack_wh(data);
            }
            else
            {
                state.gdi_image_dwh = nv20_unpack_wh(data);
            }
            return true;
        case 0x0BFC:
            state.gdi_image_xy = nv20_unpack_xy(data);
            return this.graph_gdi_prepare_words(state, is_nv3_gdi ? 0 : 1);
        case 0x13E4:
            if(!is_nv3_gdi)
            {
                return false;
            }
            state.clip_xy0 = nv20_unpack_xy(data);
            return true;
        case 0x13E8:
            if(!is_nv3_gdi)
            {
                return false;
            }
            state.clip_xy1 = nv20_unpack_xy(data);
            return true;
        case 0x13EC:
            if(!is_nv3_gdi)
            {
                return false;
            }
            state.gdi_bg_color = data >>> 0;
            return true;
        case 0x13F0:
            if(!is_nv3_gdi)
            {
                return false;
            }
            state.gdi_fg_color = data >>> 0;
            return true;
        case 0x13F4:
            if(!is_nv3_gdi)
            {
                return false;
            }
            state.gdi_image_swh = nv20_unpack_wh(data);
            return true;
        case 0x13F8:
            if(!is_nv3_gdi)
            {
                return false;
            }
            state.gdi_image_dwh = nv20_unpack_wh(data);
            return true;
        case 0x13FC:
            if(!is_nv3_gdi)
            {
                return false;
            }
            state.gdi_image_xy = nv20_unpack_xy(data);
            return this.graph_gdi_prepare_words(state, 1);
        case 0x0FF4:
            state.clip_xy0 = nv20_unpack_xy(data);
            return true;
        case 0x0FF8:
            state.clip_xy1 = nv20_unpack_xy(data);
            return true;
        case 0x0FFC:
            state.gdi_fg_color = data >>> 0;
            return true;
        default:
            if(method >= 0x0400 && method < 0x0500)
            {
                if((method & 4) === 0)
                {
                    state.rect_xy = nv20_unpack_yx(data);
                }
                else
                {
                    const size = nv20_unpack_hw(data);
                    this.graph_fill_rect(state.surface, state.rect_xy.x, state.rect_xy.y,
                                         size.w, size.h, state.color, state.color_format, state.clip,
                                         this.graph_effective_rop(state, state.operation),
                                         state);
                    this.graph_note_2d("gdi_rect", state, {
                        x: state.rect_xy.x,
                        y: state.rect_xy.y,
                        w: size.w,
                        h: size.h,
                        op: state.operation >>> 0,
                        rop: this.graph_effective_rop(state, state.operation),
                    });
                }

                return true;
            }

            if(method >= 0x0600 && method < 0x0700)
            {
                if((method & 4) === 0)
                {
                    state.rect_xy0 = nv20_unpack_xy(data);
                }
                else
                {
                    const rect_xy1 = nv20_unpack_xy(data);
                    const clip = {
                        clip_point: state.clip_xy0,
                        clip_size: {
                            w: state.clip_xy1.x - state.clip_xy0.x,
                            h: state.clip_xy1.y - state.clip_xy0.y,
                        },
                    };

                    this.graph_fill_rect(state.surface,
                                         state.rect_xy0.x, state.rect_xy0.y,
                                         rect_xy1.x - state.rect_xy0.x,
                                         rect_xy1.y - state.rect_xy0.y,
                                         state.color, state.color_format, clip,
                                         this.graph_effective_rop(state, state.operation),
                                         state);
                    this.graph_note_2d("gdi_rect", state, {
                        x: state.rect_xy0.x,
                        y: state.rect_xy0.y,
                        w: rect_xy1.x - state.rect_xy0.x,
                        h: rect_xy1.y - state.rect_xy0.y,
                        op: state.operation >>> 0,
                        rop: this.graph_effective_rop(state, state.operation),
                    });
                }

                return true;
            }

            if(method >= 0x0800 && method < 0x0A00)
            {
                if(!is_nv4_gdi)
                {
                    return false;
                }
                return this.graph_gdi_image_word(state, data, 0);
            }

            if(method >= 0x0C00 && method < 0x0E00)
            {
                return this.graph_gdi_image_word(state, data, is_nv3_gdi ? 0 : 1);
            }

            if(method >= 0x1400 && method < 0x1600)
            {
                if(!is_nv3_gdi)
                {
                    return false;
                }
                return this.graph_gdi_image_word(state, data, 1);
            }

            return false;
    }
};

NV20GeForce.prototype.graph_submit_blit = function(channel, state, method, data)
{
    switch(method)
    {
        case 0x0184:
            return this.graph_bind_context_handle(channel, state, data);
        case 0x0188:
        case 0x018C:
        case 0x0190:
        case 0x0194:
            return this.graph_bind_context_handle(channel, state, data);
        case 0x0198:
        case 0x019C:
            return this.graph_bind_surface_handle(channel, state, data);
        case 0x02FC:
            state.operation = data;
            return true;
        case 0x0300:
            state.point_in = nv20_unpack_xy(data);
            return true;
        case 0x0304:
            state.point_out = nv20_unpack_xy(data);
            return true;
        case 0x0308:
            state.size = nv20_unpack_wh(data);
            this.graph_blit(state.surface,
                            state.point_in.x, state.point_in.y,
                            state.point_out.x, state.point_out.y,
                            state.size.w, state.size.h, state.clip,
                            this.graph_effective_rop(state, state.operation),
                            state);
            return true;
        default:
            return false;
    }
};

NV20GeForce.prototype.graph_submit_ifc = function(channel, state, method, data)
{
    switch(method)
    {
        case 0x0184:
            return this.graph_bind_context_handle(channel, state, data);
        case 0x0188:
        case 0x018C:
        case 0x0190:
        case 0x0194:
            return this.graph_bind_context_handle(channel, state, data);
        case 0x0198:
        case 0x019C:
        case 0x01A0:
            return this.graph_bind_context_handle(channel, state, data);
        case 0x02F8:
        case 0x02FC:
            state.operation = data;
            return true;
        case 0x0300:
            state.color_format = data;
            state.color_format_set = true;
            state.ifc_indexed_format = false;
            state.image_upload = null;
            return true;
        case 0x0304:
            state.point = nv20_unpack_xy(data);
            state.image_upload = null;
            return true;
        case 0x0308:
            state.size_out = nv20_unpack_wh(data);
            state.image_upload = null;
            return true;
        case 0x030C:
            state.size_in = nv20_unpack_wh(data);
            state.image_upload = null;
            return true;
        case 0x03E0:
            state.operation = data;
            return true;
        case 0x03E4:
            state.ifc_index_format = data >>> 0;
            return true;
        case 0x03E8:
            state.color_format = data;
            state.color_format_set = true;
            state.ifc_indexed_format = true;
            state.image_upload = null;
            return true;
        case 0x03EC:
            state.mono_color0 = data >>> 0;
            return true;
        case 0x03F0:
        case 0x03F4:
        case 0x03F8:
        case 0x03FC:
            state.mono_color1 = data >>> 0;
            state.color = data >>> 0;
            return true;
        default:
            if(method >= 0x0400 && method < 0x2000)
            {
                return this.graph_upload_data_word(state, data);
            }

            return false;
    }
};

NV20GeForce.prototype.graph_submit_iifc = function(channel, state, method, data)
{
    switch(method)
    {
        case 0x0184:
            state.iifc_palette_handle = data >>> 0;
            state.iifc_palette_dma = this.graph_dma_object_from_handle(channel, data);
            return true;
        case 0x0188:
        case 0x018C:
        case 0x0190:
        case 0x0194:
        case 0x0198:
        case 0x019C:
        case 0x01A0:
            return this.graph_bind_context_handle(channel, state, data);
        case 0x02F8:
        case 0x02FC:
        case 0x03E0:
        case 0x03E4:
            state.iifc_operation = data >>> 0;
            state.operation = data >>> 0;
            return true;
        case 0x03E8:
            state.iifc_color_format = data >>> 0;
            state.color_format = data >>> 0;
            state.color_format_set = true;
            return true;
        case 0x03EC:
            state.iifc_bpp4 = !!data;
            return true;
        case 0x03F0:
            state.iifc_palette_offset = data >>> 0;
            return true;
        case 0x03F4:
            state.iifc_yx = nv20_unpack_xy(data);
            return true;
        case 0x03F8:
            state.iifc_dhw = nv20_unpack_wh(data);
            return true;
        case 0x03FC:
            state.iifc_shw = nv20_unpack_wh(data);
            this.graph_begin_iifc_upload(state);
            return true;
        default:
            if(method >= 0x0400 && method < 0x2000)
            {
                if(!state.iifc_words || !state.iifc_words_left)
                {
                    return false;
                }

                state.iifc_words[state.iifc_words_ptr++] = data >>> 0;
                state.iifc_words_left--;

                if(!state.iifc_words_left)
                {
                    const handled = this.graph_execute_iifc(state);
                    state.iifc_words = null;
                    state.iifc_words_ptr = 0;
                    return handled;
                }

                return true;
            }

            return false;
    }
};

NV20GeForce.prototype.graph_submit_sifc = function(channel, state, method, data)
{
    switch(method)
    {
        case 0x0184:
            return true;
        case 0x0188:
        case 0x018C:
        case 0x0190:
            return this.graph_bind_context_handle(channel, state, data);
        case 0x0194:
        case 0x0198:
            return this.graph_bind_context_handle(channel, state, data);
        case 0x02F8:
        case 0x02FC:
            state.operation = data;
            return true;
        case 0x0300:
            state.color_format = data;
            state.color_format_set = true;
            state.image_upload = null;
            return true;
        case 0x0304:
            state.size_in = nv20_unpack_wh(data);
            state.image_upload = null;
            return true;
        case 0x0308:
            state.dx_du = data;
            return true;
        case 0x030C:
            state.dy_dv = data;
            return true;
        case 0x0310:
            state.clip_point = nv20_unpack_xy(data);
            state.image_upload = null;
            return true;
        case 0x0314:
            state.clip_size = nv20_unpack_wh(data);
            state.image_upload = null;
            return true;
        case 0x0318:
            state.point12d4 = nv20_unpack_xy12d4(data);
            state.image_upload = null;
            return true;
        case 0x03EC:
            state.mono_color0 = data >>> 0;
            return true;
        case 0x03F0:
        case 0x03F4:
        case 0x03F8:
        case 0x03FC:
            state.mono_color1 = data >>> 0;
            state.color = data >>> 0;
            return true;
        default:
            if(method >= 0x0400 && method < 0x2000)
            {
                return this.graph_upload_data_word(state, data);
            }

            return false;
    }
};

NV20GeForce.prototype.graph_submit_texupload = function(channel, state, method, data)
{
    switch(method)
    {
        case 0x0184:
            return this.graph_bind_surface_handle(channel, state, data);
        case 0x0300:
            state.color_format = data;
            state.color_format_set = true;
            state.image_upload = null;
            return true;
        case 0x0304:
            state.point = nv20_unpack_xy(data);
            state.image_upload = null;
            return true;
        case 0x0308:
            state.size = nv20_unpack_wh(data);
            state.image_upload = null;
            return true;
        case 0x030C:
            state.tex_clip_point = {
                x: data & 0xFFFF,
                y: state.tex_clip_point.y,
            };
            state.tex_clip_size = {
                w: data >>> 16,
                h: state.tex_clip_size.h,
            };
            return true;
        case 0x0310:
            state.tex_clip_point = {
                x: state.tex_clip_point.x,
                y: data & 0xFFFF,
            };
            state.tex_clip_size = {
                w: state.tex_clip_size.w,
                h: data >>> 16,
            };
            return true;
        default:
            if(method >= 0x0400 && method < 0x2000)
            {
                if(state.swizzled_surface)
                {
                    return this.graph_upload_swizzled_data_word(state, data);
                }

                return this.graph_upload_data_word(state, data);
            }

            return false;
    }
};

NV20GeForce.prototype.graph_submit_m2mf = function(channel, state, method, data)
{
    switch(method)
    {
        case 0x0184:
            state.m2mf_in_dma = this.graph_dma_object_from_handle(channel, data);
            this.graph_m2mf_changed(state);
            return true;
        case 0x0188:
            state.m2mf_out_dma = this.graph_dma_object_from_handle(channel, data);
            this.graph_m2mf_changed(state);
            return true;
        case 0x030C:
            state.m2mf_offset_in = data >>> 0;
            this.graph_m2mf_changed(state);
            return true;
        case 0x0310:
            state.m2mf_offset_out = data >>> 0;
            this.graph_m2mf_changed(state);
            return true;
        case 0x0314:
            state.m2mf_pitch_in = data >>> 0;
            this.graph_m2mf_changed(state);
            return true;
        case 0x0318:
            state.m2mf_pitch_out = data >>> 0;
            this.graph_m2mf_changed(state);
            return true;
        case 0x031C:
            state.m2mf_line_length = data >>> 0;
            this.graph_m2mf_changed(state);
            return true;
        case 0x0320:
            state.m2mf_line_count = data >>> 0;
            this.graph_m2mf_changed(state);
            return true;
        case 0x0324:
            state.m2mf_format = data >>> 0;
            this.graph_m2mf_changed(state);
            return true;
        case 0x0328:
            state.m2mf_buffer_notify = data >>> 0;
            state.m2mf_executed = false;
            this.graph_execute_m2mf(state);
            this.graph_write_notify(state, 0x10);
            return true;
        default:
            return false;
    }
};

NV20GeForce.prototype.graph_submit_clip = function(state, method, data)
{
    switch(method)
    {
        case 0x0300:
            state.clip_point = nv20_unpack_xy(data);
            return true;
        case 0x0304:
            state.clip_size = nv20_unpack_wh(data);
            return true;
        default:
            return false;
    }
};

NV20GeForce.prototype.graph_submit_rop = function(state, method, data)
{
    if(method === 0x0300)
    {
        state.rop = data >>> 0;
        return true;
    }

    return false;
};

NV20GeForce.prototype.graph_submit_pattern = function(state, method, data)
{
    switch(method)
    {
        case 0x0300:
            state.pattern_shape = data >>> 0;
            return true;
        case 0x0304:
            state.pattern_color_type = data >>> 0;
            return true;
        case 0x0308:
            state.pattern_shape = data >>> 0;
            return true;
        case 0x030C:
            state.pattern_color_type = data >>> 0;
            return true;
        case 0x0310:
            state.pattern_bg_color = data >>> 0;
            return true;
        case 0x0314:
            state.pattern_fg_color = data >>> 0;
            return true;
        case 0x0318:
        case 0x031C:
            state.pattern_mono[method === 0x031C ? 1 : 0] = data >>> 0;
            return true;
        default:
            if(method >= 0x0400 && method < 0x0440)
            {
                const index = method - 0x0400 >>> 2;
                state.pattern_color[index * 4] = data & 0xFF;
                state.pattern_color[index * 4 + 1] = data >>> 8 & 0xFF;
                state.pattern_color[index * 4 + 2] = data >>> 16 & 0xFF;
                state.pattern_color[index * 4 + 3] = data >>> 24;
                return true;
            }

            if(method >= 0x0500 && method < 0x0580)
            {
                const index = method - 0x0500 >>> 2;
                state.pattern_color[index * 2] = data & 0xFFFF;
                state.pattern_color[index * 2 + 1] = data >>> 16;
                return true;
            }

            if(method >= 0x0600 && method < 0x0700)
            {
                state.pattern_color[method - 0x0600 >>> 2] = data >>> 0;
                return true;
            }

            if(method >= 0x0700 && method < 0x0800)
            {
                state.pattern_color[method - 0x0700 >>> 2] = data >>> 0;
                return true;
            }

            return false;
    }
};

NV20GeForce.prototype.graph_submit_chroma = function(state, method, data)
{
    if(method === 0x0300)
    {
        state.chroma_color_format = data >>> 0;
        return true;
    }

    if(method === 0x0304)
    {
        state.chroma_color = data >>> 0;
        state.chroma_color_set = true;
        return true;
    }

    return false;
};

NV20GeForce.prototype.graph_submit_beta = function(state, method, data)
{
    if(method === 0x0300)
    {
        state.beta = data >>> 0;
        return true;
    }

    return false;
};

NV20GeForce.prototype.graph_execute_sifm_swizzled = function(state, surface)
{
    const width = state.sifm_dhw.w || state.sifm_shw.w;
    const height = state.sifm_dhw.h || state.sifm_shw.h;

    if(!surface || !width || !height)
    {
        return false;
    }

    const start_ms = nv20_now_ms();
    const src_bpp = nv20_sifm_bpp_from_format(state.sifm_color_format, this.render_bpp);
    const src_bytes = nv20_render_bytes_per_pixel(src_bpp);
    const dst_bpp = nv20_swizzled_surface_bpp_from_format(surface.swz_format, this.render_bpp);
    const src_pitch = (state.sifm_sfmt & 0xFFFF) || (state.sifm_shw.w || width) * src_bytes;
    const dudx = state.sifm_dudx ? state.sifm_dudx | 0 : 0x00100000;
    const dvdy = state.sifm_dvdy ? state.sifm_dvdy | 0 : 0x00100000;
    const unscaled = dudx === 0x00100000 && dvdy === 0x00100000;
    const syx_raw = state.sifm_syx_raw >>> 0;
    const unscaled_src_x0 = (syx_raw & 0xFFFF) >>> 4;
    const unscaled_src_y0 = ((syx_raw >>> 16) & 0xFFFF) >>> 4;
    var scaled_src_x0 = ((syx_raw & 0xFFFF) << 16) - 0x80000;
    var scaled_src_y = ((syx_raw & 0xFFFF0000) | 0) - 0x80000;

    if(scaled_src_x0 < 0)
    {
        scaled_src_x0 = 0;
    }

    if(scaled_src_y < 0)
    {
        scaled_src_y = 0;
    }

    for(var y = 0; y < height; y++)
    {
        const src_y = unscaled ?
            unscaled_src_y0 + y :
            scaled_src_y >> 20;
        var scaled_src_x = scaled_src_x0;
        const row_offset = state.sifm_sofs + src_y * src_pitch >>> 0;
        const row_location = this.graph_dma_linear_location(state.sifm_src_dma, row_offset, false);

        for(var x = 0; x < width; x++)
        {
            const src_x = unscaled ?
                unscaled_src_x0 + x :
                scaled_src_x >> 20;
            const src_row_offset = src_x * src_bytes >>> 0;
            const src_offset = row_offset + src_row_offset >>> 0;
            var color = row_location ?
                this.graph_read_linear_pixel(row_location, src_row_offset, src_bytes) :
                this.graph_read_dma_pixel(state.sifm_src_dma, src_offset, src_bytes, false);

            if((state.sifm_color_format & 0xFF) === 4)
            {
                color = color | 0xFF000000;
            }

            const bytes = nv20_color_to_bytes(color >>> 0,
                                              src_bpp,
                                              state.sifm_color_format,
                                              dst_bpp,
                                              surface.swz_format);
            this.graph_write_swizzled_surface_pixel(surface,
                                                    state.sifm_dyx.x + x,
                                                    state.sifm_dyx.y + y,
                                                    bytes);

            if(!unscaled)
            {
                scaled_src_x += dudx;
            }
        }

        if(!unscaled)
        {
            scaled_src_y += dvdy;
        }
    }

    this["graph_accel_count"]++;
    this.graph_note_2d("sifm_swizzle", state, {
        x: state.sifm_dyx.x,
        y: state.sifm_dyx.y,
        w: width,
        h: height,
        src_bpp: src_bpp,
        dst_bpp: dst_bpp,
        fmt: state.sifm_color_format >>> 0,
        pitch: src_pitch,
        ms: nv20_now_ms() - start_ms,
    });
    return true;
};

NV20GeForce.prototype.graph_execute_sifm = function(state)
{
    if(state.swizzled_surface)
    {
        return this.graph_execute_sifm_swizzled(state, state.swizzled_surface);
    }

    const surface = state.surface || state.sifm_surface || this.graph_default_surface();
    const width = state.sifm_dhw.w || state.sifm_shw.w;
    const height = state.sifm_dhw.h || state.sifm_shw.h;

    if(!width || !height)
    {
        return false;
    }

    const start_ms = nv20_now_ms();
    const src_bpp = nv20_sifm_bpp_from_format(state.sifm_color_format, this.render_bpp);
    const src_bytes = nv20_render_bytes_per_pixel(src_bpp);
    const dst_bpp = nv20_surface_bpp_from_format(surface.format, this.render_bpp);
    this.update_render_mode_from_surface(surface, "2d-scaled-image",
                                         state.sifm_dyx.x + width,
                                         state.sifm_dyx.y + height);
    const src_pitch = (state.sifm_sfmt & 0xFFFF) || (state.sifm_shw.w || width) * src_bytes;
    const dudx = state.sifm_dudx ? state.sifm_dudx | 0 : 0x00100000;
    const dvdy = state.sifm_dvdy ? state.sifm_dvdy | 0 : 0x00100000;
    const unscaled = dudx === 0x00100000 && dvdy === 0x00100000;
    const rop = this.graph_effective_rop(state, state.sifm_operation);
    const syx_raw = state.sifm_syx_raw >>> 0;
    const unscaled_src_x0 = (syx_raw & 0xFFFF) >>> 4;
    const unscaled_src_y0 = ((syx_raw >>> 16) & 0xFFFF) >>> 4;
    var scaled_src_x0 = ((syx_raw & 0xFFFF) << 16) - 0x80000;
    var scaled_src_y = ((syx_raw & 0xFFFF0000) | 0) - 0x80000;

    if(scaled_src_x0 < 0)
    {
        scaled_src_x0 = 0;
    }

    if(scaled_src_y < 0)
    {
        scaled_src_y = 0;
    }

    const dirty = {
        dirty_min: this.vram_size,
        dirty_max: 0,
    };

    for(var y = 0; y < height; y++)
    {
        const src_y = unscaled ?
            unscaled_src_y0 + y :
            scaled_src_y >> 20;
        var scaled_src_x = scaled_src_x0;
        const row_offset = state.sifm_sofs + src_y * src_pitch >>> 0;
        const row_location = this.graph_dma_linear_location(state.sifm_src_dma, row_offset, false);

        for(var x = 0; x < width; x++)
        {
            const src_x = unscaled ?
                unscaled_src_x0 + x :
                scaled_src_x >> 20;
            const src_row_offset = src_x * src_bytes >>> 0;
            const src_offset = row_offset + src_row_offset >>> 0;
            var color = row_location ?
                this.graph_read_linear_pixel(row_location, src_row_offset, src_bytes) :
                this.graph_read_dma_pixel(state.sifm_src_dma, src_offset, src_bytes, false);

            if((state.sifm_color_format & 0xFF) === 4)
            {
                color = color | 0xFF000000;
            }

            const src_color = color >>> 0;
            const bytes = nv20_color_to_bytes(src_color,
                                              src_bpp,
                                              state.sifm_color_format,
                                              dst_bpp,
                                              surface.format);

            this.graph_write_surface_pixel(surface,
                                           state.sifm_dyx.x + x,
                                           state.sifm_dyx.y + y,
                                           bytes,
                                           rop,
                                           null,
                                           {
                                               operation: state.sifm_operation >>> 0,
                                               state: state,
                                               src_color: src_color,
                                               src_bpp: src_bpp,
                                               src_format: state.sifm_color_format >>> 0,
                                               dst_bpp: dst_bpp,
                                               defer_mark: dirty,
                                           });

            if(!unscaled)
            {
                scaled_src_x += dudx;
            }
        }

        if(!unscaled)
        {
            scaled_src_y += dvdy;
        }
    }

    this.graph_flush_vram_mark(dirty);
    this["graph_accel_count"]++;
    this.graph_note_2d("sifm", state, {
        x: state.sifm_dyx.x,
        y: state.sifm_dyx.y,
        w: width,
        h: height,
        src_bpp: src_bpp,
        dst_bpp: dst_bpp,
        fmt: state.sifm_color_format >>> 0,
        pitch: src_pitch,
        op: state.sifm_operation >>> 0,
        rop: rop,
        ms: nv20_now_ms() - start_ms,
    });
    return true;
};

NV20GeForce.prototype.graph_submit_sifm = function(channel, state, method, data)
{
    switch(method)
    {
        case 0x0184:
            state.sifm_src_dma = this.graph_dma_object_from_handle(channel, data);
            return true;
        case 0x0188:
        case 0x018C:
        case 0x0190:
            return this.graph_bind_context_handle(channel, state, data);
        case 0x0194:
        case 0x0198:
        case 0x019C:
            return this.graph_bind_context_handle(channel, state, data);
        case 0x0300:
            state.sifm_color_format = data >>> 0;
            return true;
        case 0x0304:
            state.sifm_operation = data >>> 0;
            return true;
        case 0x0310:
            state.sifm_dyx = nv20_unpack_xy(data);
            return true;
        case 0x0314:
            state.sifm_dhw = nv20_unpack_wh(data);
            return true;
        case 0x0318:
            state.sifm_dudx = data | 0;
            return true;
        case 0x031C:
            state.sifm_dvdy = data | 0;
            return true;
        case 0x0400:
            state.sifm_shw = nv20_unpack_wh(data);
            return true;
        case 0x0404:
            state.sifm_sfmt = data >>> 0;
            return true;
        case 0x0408:
            state.sifm_sofs = data >>> 0;
            return true;
        case 0x040C:
            state.sifm_syx = nv20_unpack_xy12d4(data);
            state.sifm_syx_raw = data >>> 0;
            this.graph_execute_sifm(state);
            return true;
        default:
            return false;
    }
};

NV20GeForce.prototype.graph_d3d_init_defaults = function(state)
{
    const class_id = state.class_id & 0xFFFF;

    state.d3d_attrib_count = class_id === NV20_CLASS_D3D_NV10 ? 8 : 16;
    state.d3d_tex_coord_count = class_id === NV20_CLASS_D3D_NV10 ? 2 :
        class_id === NV20_CLASS_D3D_NV10_TCL ? 4 : 8;
    state.d3d_window_offset_x = class_id === NV20_CLASS_D3D_NV10 ? 2048 : 0;
    state.d3d_window_offset_y = class_id === NV20_CLASS_D3D_NV10 ? 2048 : 0;
    state.d3d_vertex_arrays = [];
    state.d3d_vertex_data_imm = [];
    state.d3d_transform_program = [];
    state.d3d_transform_program_load = 0;
    state.d3d_transform_program_start = 0;
    state.d3d_transform_constant = [];
    state.d3d_transform_constant_load = 0;
    state.d3d_begin_end = 0;
};

NV20GeForce.prototype.graph_d3d_note_method = function(state, method, data)
{
    if(!state.d3d_methods)
    {
        state.d3d_methods = {};
    }

    state.d3d_methods[h(method & 0x1FFC, 4)] = data >>> 0;
};

NV20GeForce.prototype.graph_d3d_texture_state = function(state, index)
{
    index &= 0xF;
    var texture = state.d3d_textures[index];

    if(!texture)
    {
        texture = {
            offset: 0,
            dma: null,
            dma_select: 0,
            cubemap: false,
            format: 0,
            levels: 0,
            base_size: [0, 0, 0],
            wrap: [0, 0, 0],
            control0: 0,
            control1: 0,
            control3: 0,
            enabled: false,
            filter: 0,
            image_rect: 0,
            key_color: 0,
            palette: 0,
            offset_matrix: [0, 0, 0, 0],
            methods: {},
        };
        state.d3d_textures[index] = texture;
    }

    return texture;
};

NV20GeForce.prototype.graph_d3d_vertex_array_state = function(state, index)
{
    index &= 0xF;
    var array = state.d3d_vertex_arrays[index];

    if(!array)
    {
        array = {
            offset: 0,
            format: 0,
            stride: 0,
            type: 0,
            size: 0,
            dx: false,
            homogeneous: false,
        };
        state.d3d_vertex_arrays[index] = array;
    }

    return array;
};

NV20GeForce.prototype.graph_d3d_vector_state = function(list, index)
{
    index >>>= 0;
    var vector = list[index];

    if(!vector)
    {
        vector = [0, 0, 0, 0];
        list[index] = vector;
    }

    return vector;
};

NV20GeForce.prototype.graph_d3d_set_vertex_array_format = function(array, value)
{
    value >>>= 0;
    array.format = value;
    array.stride = value >>> 8 & 0xFF;
    array.dx = (value & 0x00010000) !== 0;
    array.homogeneous = (value & 0x01000000) !== 0;

    if(!array.dx)
    {
        array.type = value & 0xF;
        array.size = value >>> 4 & 0xF;
        return;
    }

    switch(value & 0xFF)
    {
        case 0x44:
            array.type = 4;
            array.size = 4;
            break;
        case 0x99:
            array.type = 2;
            array.size = 2;
            break;
        case 0xAA:
            array.type = 2;
            array.size = 3;
            break;
        case 0xBB:
            array.type = 2;
            array.size = 4;
            break;
        case 0xCC:
            array.type = 0;
            array.size = 4;
            break;
    }
};

NV20GeForce.prototype.graph_d3d_record_texture_method = function(state, word, data)
{
    const class_id = state.class_id & 0xFFFF;
    var method_offset = 0;
    var texture_index = 0;
    var texture_method = 0;

    if(class_id === NV20_CLASS_D3D_NV10 && word >= 0x086 && word <= 0x095)
    {
        method_offset = word - 0x086;
        texture_index = method_offset & 1;
        texture_method = method_offset >>> 1;
    }
    else if(class_id === NV20_CLASS_D3D_NV10_TCL && word >= 0x6C0 && word <= 0x6FF)
    {
        method_offset = word - 0x6C0;
        texture_index = method_offset >>> 4;
        texture_method = method_offset & 0xF;
    }
    else if(class_id >= NV20_CLASS_D3D_NV15 && word >= 0x680 && word <= 0x6FF)
    {
        method_offset = word - 0x680;
        texture_index = method_offset >>> 3;
        texture_method = method_offset & 7;
    }
    else if(class_id >= NV20_CLASS_D3D_NV15 && word >= 0x610 && word <= 0x61F)
    {
        const texture = this.graph_d3d_texture_state(state, word & 0xF);
        texture.control3 = data >>> 0;
        texture.methods["control3"] = data >>> 0;
        return true;
    }
    else if(class_id >= NV20_CLASS_D3D_NV15 && word >= 0x740 && word <= 0x74F)
    {
        const texture = this.graph_d3d_texture_state(state, word - 0x740);
        texture.key_color = data >>> 0;
        texture.methods["key_color"] = data >>> 0;
        return true;
    }
    else
    {
        return false;
    }

    const texture = this.graph_d3d_texture_state(state, texture_index);
    texture.methods[h(texture_method, 2)] = data >>> 0;

    if(texture_method === 0)
    {
        texture.offset = data >>> 0;
    }
    else if(texture_method === 1)
    {
        texture.dma_select = data & 3;
        texture.dma = (data & 3) === 1 ? state.d3d_a_dma : state.d3d_b_dma;
        texture.cubemap = (data & 4) !== 0;

        if(class_id === NV20_CLASS_D3D_NV10)
        {
            texture.format = data >>> 7 & 0x1F;
            texture.levels = data >>> 12 & 0xF;
            texture.base_size[0] = data >>> 16 & 0xF;
            texture.base_size[1] = data >>> 20 & 0xF;
            texture.wrap[0] = data >>> 24 & 0xF;
            texture.wrap[1] = data >>> 28 & 0xF;
        }
        else
        {
            texture.format = data >>> 8 & 0xFF;
            texture.levels = data >>> 16 & 0xF;
            texture.base_size[0] = data >>> 20 & 0xF;
            texture.base_size[1] = data >>> 24 & 0xF;
            texture.base_size[2] = data >>> 28 & 0xF;
        }
    }
    else if(texture_method === 2 && class_id !== NV20_CLASS_D3D_NV10)
    {
        texture.wrap[0] = data & 0xF;
        texture.wrap[1] = data >>> 8 & 0xF;
        texture.wrap[2] = data >>> 16 & 0xF;
    }
    else if((texture_method === 2 && class_id === NV20_CLASS_D3D_NV10) ||
            (texture_method === 3 && class_id !== NV20_CLASS_D3D_NV10))
    {
        texture.control0 = data >>> 0;
        texture.enabled = (data >>> 30 & 1) !== 0;
    }
    else if((texture_method === 3 && class_id === NV20_CLASS_D3D_NV10) ||
            (texture_method === 4 && class_id !== NV20_CLASS_D3D_NV10))
    {
        texture.control1 = data >>> 0;
    }
    else if((texture_method === 6 && class_id === NV20_CLASS_D3D_NV10) ||
            (texture_method === 5 && class_id !== NV20_CLASS_D3D_NV10))
    {
        texture.filter = data >>> 0;
    }
    else if((texture_method === 5 && class_id === NV20_CLASS_D3D_NV10) ||
            (texture_method === 7 && class_id === NV20_CLASS_D3D_NV10_TCL) ||
            (texture_method === 6 && class_id >= NV20_CLASS_D3D_NV15))
    {
        texture.image_rect = data >>> 0;
    }
    else if((texture_method === 7 && class_id === NV20_CLASS_D3D_NV10) ||
            (texture_method === 8 && class_id === NV20_CLASS_D3D_NV10_TCL))
    {
        texture.palette = data >>> 0;
    }
    else if(texture_method >= 10 && texture_method <= 13 && class_id === NV20_CLASS_D3D_NV10_TCL)
    {
        texture.offset_matrix[texture_method - 10] = data >>> 0;
    }

    return true;
};

NV20GeForce.prototype.graph_d3d_record_vertex_array_method = function(state, word, data)
{
    const class_id = state.class_id & 0xFFFF;
    var array_index = -1;
    var is_format = false;

    if(word === 0x5CF && class_id > NV20_CLASS_D3D_NV15)
    {
        state.d3d_vertex_data_base_index = data >>> 0;
        return true;
    }

    if(class_id === NV20_CLASS_D3D_NV10 && word >= 0x340 && word <= 0x34F)
    {
        const method_offset = word - 0x340;
        array_index = method_offset >>> 1;
        is_format = (method_offset & 1) !== 0;
    }
    else if(class_id === NV20_CLASS_D3D_NV10_TCL && word >= 0x5C8 && word <= 0x5D7)
    {
        array_index = word - 0x5C8;
    }
    else if(class_id === NV20_CLASS_D3D_NV10_TCL && word >= 0x5D8 && word <= 0x5E7)
    {
        array_index = word - 0x5D8;
        is_format = true;
    }
    else if(class_id >= NV20_CLASS_D3D_NV15 && word >= 0x5A0 && word <= 0x5AF)
    {
        array_index = word - 0x5A0;
    }
    else if(class_id >= NV20_CLASS_D3D_NV15 && word >= 0x5D0 && word <= 0x5DF)
    {
        array_index = word - 0x5D0;
        is_format = true;
    }
    else
    {
        return false;
    }

    const array = this.graph_d3d_vertex_array_state(state, array_index);

    if(is_format)
    {
        this.graph_d3d_set_vertex_array_format(array, data);
    }
    else
    {
        array.offset = data >>> 0;
    }

    return true;
};

NV20GeForce.prototype.graph_d3d_record_transform_method = function(state, word, data)
{
    const class_id = state.class_id & 0xFFFF;

    if((class_id === NV20_CLASS_D3D_NV10_TCL && word >= 0x2C0 && word <= 0x2C3) ||
       (class_id >= NV20_CLASS_D3D_NV15 && word >= 0x2E0 && word <= 0x2E3))
    {
        const vector = this.graph_d3d_vector_state(
            state.d3d_transform_program,
            state.d3d_transform_program_load);
        const component = word & 3;
        vector[component] = data >>> 0;

        if(component === 3)
        {
            state.d3d_transform_program_load++;
        }

        return true;
    }

    if((class_id === NV20_CLASS_D3D_NV10_TCL && word >= 0x2E0 && word <= 0x2E3) ||
       (class_id >= NV20_CLASS_D3D_NV15 && word >= 0x7C0 && word <= 0x7CF))
    {
        const vector = this.graph_d3d_vector_state(
            state.d3d_transform_constant,
            state.d3d_transform_constant_load);
        const component = word & 3;
        vector[component] = data >>> 0;

        if(component === 3)
        {
            state.d3d_transform_constant_load++;
        }

        return true;
    }

    if(word === 0x7A7)
    {
        state.d3d_transform_program_load = data >>> 0;
        return true;
    }

    if(word === 0x7A8)
    {
        state.d3d_transform_program_start = data >>> 0;
        return true;
    }

    if((word === 0x7A9 && class_id === NV20_CLASS_D3D_NV10_TCL) ||
       (word === 0x7BF && class_id >= NV20_CLASS_D3D_NV15))
    {
        state.d3d_transform_constant_load = data >>> 0;
        return true;
    }

    return false;
};

NV20GeForce.prototype.graph_d3d_record_immediate_vertex_method = function(state, word, data)
{
    const class_id = state.class_id & 0xFFFF;
    var attrib_index = -1;
    var component = 0;

    if(class_id >= NV20_CLASS_D3D_NV15 && word >= 0x540 && word <= 0x57F)
    {
        attrib_index = word >>> 2 & 0xF;
        component = word & 3;
    }
    else if(class_id >= NV20_CLASS_D3D_NV10_TCL && word >= 0x620 && word <= 0x63F)
    {
        attrib_index = word >>> 1 & 0xF;
        component = word & 1;
    }
    else if(class_id >= NV20_CLASS_D3D_NV10_TCL && word >= 0x640 && word <= 0x64F)
    {
        attrib_index = word & 0xF;
        const vector = this.graph_d3d_vector_state(state.d3d_vertex_data_imm, attrib_index);
        vector[0] = data & 0xFFFF;
        vector[1] = data >>> 16;
        vector[2] = 0;
        vector[3] = 0x3F800000;
        return true;
    }
    else if(class_id >= NV20_CLASS_D3D_NV10_TCL && word >= 0x650 && word <= 0x65F)
    {
        attrib_index = word & 0xF;
        const vector = this.graph_d3d_vector_state(state.d3d_vertex_data_imm, attrib_index);
        vector[0] = data >>> 0;
        return true;
    }
    else if((class_id === NV20_CLASS_D3D_NV10_TCL && word >= 0x680 && word <= 0x6BF) ||
            (class_id >= NV20_CLASS_D3D_NV15 && word >= 0x700 && word <= 0x73F))
    {
        attrib_index = word >>> 2 & 0xF;
        component = word & 3;
    }
    else
    {
        return false;
    }

    this.graph_d3d_vector_state(state.d3d_vertex_data_imm, attrib_index)[component] = data >>> 0;
    return true;
};

NV20GeForce.prototype.graph_d3d_record_state_method = function(channel, state, method, data)
{
    const word = method >>> 2;
    const class_id = state.class_id & 0xFFFF;

    if(word === 0x000)
    {
        this.graph_d3d_init_defaults(state);
        return true;
    }

    if(this.graph_d3d_record_texture_method(state, word, data) ||
       this.graph_d3d_record_vertex_array_method(state, word, data) ||
       this.graph_d3d_record_transform_method(state, word, data) ||
       this.graph_d3d_record_immediate_vertex_method(state, word, data))
    {
        return true;
    }

    switch(word)
    {
        case 0x08B:
            state.d3d_surface_pitch_z = data >>> 0;
            return true;
        case 0x0AE:
            state.d3d_window_offset_x = nv20_i16(data);
            state.d3d_window_offset_y = nv20_i16(data >>> 16);
            return true;
        case 0x230:
            state.d3d_scissor_x = data & 0xFFFF;
            state.d3d_scissor_width = data >>> 16;
            return true;
        case 0x231:
            state.d3d_scissor_y = data & 0xFFFF;
            state.d3d_scissor_height = data >>> 16;
            return true;
        case 0x239:
            state.d3d_shader_program = data >>> 0;
            return true;
        case 0x280:
            state.d3d_viewport_x = data & 0xFFFF;
            state.d3d_viewport_width = data >>> 16;
            return true;
        case 0x281:
            state.d3d_viewport_y = data & 0xFFFF;
            state.d3d_viewport_height = data >>> 16;
            return true;
        case 0x37F:
        case 0x4FF:
            if(class_id === NV20_CLASS_D3D_NV10)
            {
                state.d3d_begin_end = data >>> 0;
            }
            return true;
        case 0x5FF:
            if(class_id <= NV20_CLASS_D3D_NV10_TCL)
            {
                state.d3d_begin_end = data >>> 0;
            }
            return true;
        case 0x602:
            if(class_id >= NV20_CLASS_D3D_NV15)
            {
                state.d3d_begin_end = data >>> 0;
            }
            return true;
        case 0x607:
            state.d3d_index_array_offset = data >>> 0;
            return true;
        case 0x608:
            state.d3d_index_array_dma = data >>> 0;
            return true;
        case 0x758:
            state.d3d_shader_control = data >>> 0;
            return true;
        case 0x75D:
            this.crtc_start = data >>> 0;
            this.update_render_mode_from_crtc("d3d-crtc-start");
            return true;
        default:
            break;
    }

    if(word >= 0x0B0 && word <= 0x0BF)
    {
        const index = word >>> 1 & 7;
        var clip = state.d3d_window_clip[index];

        if(!clip)
        {
            clip = { x1: 0, x2: 0, y1: 0, y2: 0 };
            state.d3d_window_clip[index] = clip;
        }

        if((word & 1) === 0)
        {
            clip.x1 = data & 0xFFFF;
            clip.x2 = data >>> 16;
        }
        else
        {
            clip.y1 = data & 0xFFFF;
            clip.y2 = data >>> 16;
        }

        return true;
    }

    return false;
};

NV20GeForce.prototype.graph_d3d_write_report = function(state, offset)
{
    if(!state.d3d_report_dma)
    {
        return;
    }

    offset = offset >>> 0 & 0x00FFFFFF;
    const timestamp = this.timer_read64();
    this.graph_write_dma32(state.d3d_report_dma, offset, timestamp >>> 0, false);
    this.graph_write_dma32(state.d3d_report_dma, offset + 4 >>> 0,
                           Math.floor(timestamp / 0x100000000) >>> 0, false);
    this.graph_write_dma32(state.d3d_report_dma, offset + 8 >>> 0, 0, false);
    this.graph_write_dma32(state.d3d_report_dma, offset + 12 >>> 0, 0, false);
};

NV20GeForce.prototype.graph_d3d_write_semaphore = function(state, value)
{
    if(!state.d3d_semaphore_dma)
    {
        return;
    }

    this.graph_write_dma32(state.d3d_semaphore_dma,
                           state.d3d_semaphore_offset || 0,
                           value >>> 0,
                           false);
};

NV20GeForce.prototype.graph_d3d_write_dma16 = function(dma, offset, value, prefer_vram)
{
    value >>>= 0;
    this.graph_write_dma_byte(dma, offset, value, prefer_vram);
    this.graph_write_dma_byte(dma, offset + 1 >>> 0, value >>> 8, prefer_vram);
};

NV20GeForce.prototype.graph_d3d_clear_rect = function(state)
{
    var x = state.d3d_clip_horizontal & 0xFFFF;
    var y = state.d3d_clip_vertical & 0xFFFF;
    var width = state.d3d_clip_horizontal >>> 16;
    var height = state.d3d_clip_vertical >>> 16;

    if(width <= 0 || height <= 0)
    {
        return null;
    }

    if(state.d3d_scissor_width && state.d3d_scissor_height)
    {
        const max_x = x + width;
        const max_y = y + height;
        const scissor_x = state.d3d_scissor_x + state.d3d_window_offset_x | 0;
        const scissor_y = state.d3d_scissor_y + state.d3d_window_offset_y | 0;
        const scissor_max_x = scissor_x + state.d3d_scissor_width;
        const scissor_max_y = scissor_y + state.d3d_scissor_height;

        x = Math.max(x, scissor_x);
        y = Math.max(y, scissor_y);
        width = Math.min(max_x, scissor_max_x) - x;
        height = Math.min(max_y, scissor_max_y) - y;
    }

    if(width <= 0 || height <= 0)
    {
        return null;
    }

    return {
        x: x | 0,
        y: y | 0,
        w: width | 0,
        h: height | 0,
    };
};

NV20GeForce.prototype.graph_d3d_color_surface = function(state)
{
    const surface_format = nv20_surface_format_from_d3d_format(
        state.class_id,
        state.d3d_surface_format) || nv20_surface_format_from_bpp(this.render_bpp);
    const pitch = (state.d3d_surface_pitch & 0xFFFF) ||
        state.d3d_surface_pitch ||
        this.render_stride;

    return {
        class_id: state.class_id,
        class_name: (state.class_name || "d3d") + "-clear-color",
        format: surface_format,
        src_pitch: pitch,
        dst_pitch: pitch,
        src_offset: state.d3d_surface_color_offset >>> 0,
        dst_offset: state.d3d_surface_color_offset >>> 0,
        surface_format_set: true,
        surface_pitch_set: true,
        surface_src_offset_set: true,
        surface_dst_offset_set: true,
        src_dma: state.d3d_color_dma || null,
        dst_dma: state.d3d_color_dma || null,
    };
};

NV20GeForce.prototype.graph_d3d_clear_depth = function(state, rect)
{
    const clear = state.d3d_clear_surface >>> 0;
    const depth_clear = (clear & 1) !== 0;
    const stencil_clear = (clear & 2) !== 0;

    if(!depth_clear && !stencil_clear)
    {
        return;
    }

    // Depth/stencil contents are not consumed until the 3D rasterizer exists.
    // Do not spend millions of byte writes or risk touching the visible front
    // buffer when the guest is only using clears as part of a mode transition.
    this.graph_count_2d("d3d_depth_clear_skip");
};

NV20GeForce.prototype.graph_execute_d3d_clear = function(state)
{
    const rect = this.graph_d3d_clear_rect(state);

    if(!rect)
    {
        return false;
    }

    if(state.d3d_clear_surface & 0x000000F0)
    {
        const surface = this.graph_d3d_color_surface(state);
        this.graph_fill_rect(surface,
                             rect.x,
                             rect.y,
                             rect.w,
                             rect.h,
                             state.d3d_color_clear_value,
                             surface.format,
                             null,
                             NV20_ROP_SRCCOPY,
                             state);
    }

    this.graph_d3d_clear_depth(state, rect);
    return true;
};

NV20GeForce.prototype.graph_submit_d3d = function(channel, state, method, data)
{
    this["graph_d3d_method_count"]++;
    this.graph_d3d_note_method(state, method, data);
    this.graph_d3d_record_state_method(channel, state, method, data);
    const class_id = state.class_id & 0xFFFF;

    switch(method)
    {
        case 0x0120:
            this.graph_flip_read = data >>> 0;
            return true;
        case 0x0124:
            this.graph_flip_write = data >>> 0;
            return true;
        case 0x0128:
            this.graph_flip_modulo = data >>> 0 || 1;
            this.graph_flip_read %= this.graph_flip_modulo;
            this.graph_flip_write %= this.graph_flip_modulo;
            return true;
        case 0x012C:
            this.graph_flip_modulo = this.graph_flip_modulo || 2;
            this.graph_flip_write = (this.graph_flip_write + 1) % this.graph_flip_modulo;
            return true;
        case 0x0130:
            this.graph_flip_modulo = this.graph_flip_modulo || 2;
            if(this.graph_flip_read === this.graph_flip_write)
            {
                this.fifo_wait_flip = true;
                this.fifo_note_wait("flip");
            }
            return true;
        case 0x0184:
            state.d3d_a_dma = this.graph_dma_object_from_handle(channel, data);
            return true;
        case 0x0188:
            state.d3d_b_dma = this.graph_dma_object_from_handle(channel, data);
            return true;
        case 0x018C:
            state.d3d_vertex_a_dma = this.graph_dma_object_from_handle(channel, data);
            if(class_id === NV20_CLASS_D3D_NV10)
            {
                state.d3d_vertex_b_dma = state.d3d_vertex_a_dma;
            }
            return true;
        case 0x0194:
            state.d3d_color_dma = this.graph_dma_object_from_handle(channel, data);
            return true;
        case 0x0198:
            state.d3d_zeta_dma = this.graph_dma_object_from_handle(channel, data);
            return true;
        case 0x019C:
            state.d3d_vertex_a_dma = this.graph_dma_object_from_handle(channel, data);
            return true;
        case 0x01A0:
            state.d3d_vertex_b_dma = this.graph_dma_object_from_handle(channel, data);
            return true;
        case 0x01A4:
            state.d3d_semaphore_dma = this.graph_dma_object_from_handle(channel, data);
            return true;
        case 0x01A8:
            state.d3d_report_dma = this.graph_dma_object_from_handle(channel, data);
            return true;
        case 0x0200:
            state.d3d_clip_horizontal = data >>> 0;
            return true;
        case 0x0204:
            state.d3d_clip_vertical = data >>> 0;
            return true;
        case 0x0208:
            state.d3d_surface_format = data >>> 0;
            this.update_render_mode_from_d3d_surface(state, "d3d-surface-format");
            return true;
        case 0x020C:
            state.d3d_surface_pitch = data >>> 0;
            this.update_render_mode_from_d3d_surface(state, "d3d-surface-pitch");
            return true;
        case 0x0210:
            state.d3d_surface_color_offset = data >>> 0;
            this.update_render_mode_from_d3d_surface(state, "d3d-surface-color-offset");
            return true;
        case 0x0214:
            state.d3d_surface_zeta_offset = data >>> 0;
            return true;
        case 0x022C:
            state.d3d_surface_pitch_z = data >>> 0;
            return true;
        case 0x0DFC:
        case 0x13FC:
            if(class_id === NV20_CLASS_D3D_NV10)
            {
                state.d3d_begin_end = data >>> 0;
            }
            return true;
        case 0x17D0:
            if(class_id === NV20_CLASS_D3D_NV10_TCL)
            {
                this.graph_d3d_write_report(state, data);
            }
            return true;
        case 0x17FC:
            if(class_id <= NV20_CLASS_D3D_NV10_TCL)
            {
                state.d3d_begin_end = data >>> 0;
            }
            return true;
        case 0x1800:
            if(class_id >= NV20_CLASS_D3D_NV15)
            {
                this.graph_d3d_write_report(state, data);
            }
            return true;
        case 0x1808:
            if(class_id >= NV20_CLASS_D3D_NV15)
            {
                state.d3d_begin_end = data >>> 0;
            }
            return true;
        case 0x181C:
            state.d3d_index_array_offset = data >>> 0;
            return true;
        case 0x1820:
            state.d3d_index_array_dma = data >>> 0;
            return true;
        case 0x1D6C:
            state.d3d_semaphore_offset = data >>> 0;
            return true;
        case 0x1D70:
            this.graph_d3d_write_semaphore(state, data);
            return true;
        case 0x1D74:
            this.crtc_start = data >>> 0;
            this.update_render_mode_from_crtc("d3d-crtc-start");
            return true;
        case 0x1D8C:
            state.d3d_zstencil_clear_value = data >>> 0;
            return true;
        case 0x1D90:
            state.d3d_color_clear_value = data >>> 0;
            return true;
        case 0x1D94:
            state.d3d_clear_surface = data >>> 0;
            this.graph_execute_d3d_clear(state);
            return true;
        default:
            return nv20_is_d3d_like_method(method);
    }
};

NV20GeForce.prototype.graph_submit_global_method = function(channel, method, data)
{
    switch(method)
    {
        case 0x0050:
            channel.ref = data >>> 0;
            this.fifo_ref_cnt = channel.ref;
            return true;
        case 0x0060:
        {
            const object = this.fifo_ramht_lookup(channel.id, data);
            channel.semaphore_handle = data >>> 0;
            channel.semaphore_dma = object && nv20_is_dma_class(object.class_id) ?
                this.graph_dma_object_from_instance(object.instance, data, object.class_id) : null;
            return true;
        }
        case 0x0064:
            channel.semaphore_offset = data >>> 0;
            return true;
        case 0x0068:
        {
            if(!channel.semaphore_dma)
            {
                return true;
            }

            const current = this.graph_read_dma32(channel.semaphore_dma,
                                                  channel.semaphore_offset || 0,
                                                  false);

            if(current !== (data >>> 0))
            {
                // Bochs waits here until software releases the semaphore. The
                // JS bridge has no asynchronous FIFO wait path yet, so complete
                // the acquire by publishing the expected value.
                this.graph_write_dma32(channel.semaphore_dma,
                                       channel.semaphore_offset || 0,
                                       data >>> 0,
                                       false);
            }

            return true;
        }
        case 0x006C:
            if(!channel.semaphore_dma)
            {
                return true;
            }

            this.graph_write_dma32(channel.semaphore_dma,
                                   channel.semaphore_offset || 0,
                                   data >>> 0,
                                   false);
            return true;
        default:
            return false;
    }
};

NV20GeForce.prototype.graph_submit_method = function(channel, subchannel, method, data)
{
    if(this.graph_submit_global_method(channel, method, data))
    {
        return true;
    }

    var object = channel.subchannels[subchannel] || this.fifo_subchannels[subchannel];

    if(!object)
    {
        if(method === 0x0100 || method === 0x0104 || method === 0x0108 || method === 0x010C)
        {
            return true;
        }

        if(!nv20_is_d3d_like_method(method))
        {
            return false;
        }

        object = {
            handle: 0,
            context: 0,
            instance: 0,
            engine: NV20_GRAPH_OBJECT_ENGINE,
            class_id: 0,
            missing: true,
        };
        channel.subchannels[subchannel] = object;
        this.fifo_subchannels[subchannel] = object;
    }

    const state = object.state || this.graph_get_object_state(channel, object);
    const class_id = state.class_id & 0xFFFF;

    if(method === 0x0180)
    {
        state.notifier_handle = data >>> 0;
        state.notifier_dma = this.graph_dma_object_from_handle(channel, data);
        return true;
    }

    if(method === 0x0104)
    {
        state.notify_pending = true;
        state.notify_type = data >>> 0;
        return true;
    }

    var handled = false;

    if(method === 0x0100 || method === 0x0108 || method === 0x010C)
    {
        handled = true;
    }

    else if(nv20_is_surface2d_class(class_id))
    {
        handled = this.graph_submit_surface2d(channel, state, method, data);
    }
    else if(class_id === NV20_CLASS_CLIP)
    {
        handled = this.graph_submit_clip(state, method, data);
    }
    else if(nv20_is_rect_class(class_id))
    {
        handled = this.graph_submit_rect(channel, state, method, data);
    }
    else if(nv20_is_gdi_class(class_id))
    {
        handled = this.graph_submit_gdi(channel, state, method, data);
    }
    else if(nv20_is_blit_class(class_id))
    {
        handled = this.graph_submit_blit(channel, state, method, data);
    }
    else if(nv20_is_ifc_class(class_id))
    {
        handled = this.graph_submit_ifc(channel, state, method, data);
    }
    else if(nv20_is_iifc_class(class_id))
    {
        handled = this.graph_submit_iifc(channel, state, method, data);
    }
    else if(nv20_is_sifc_class(class_id))
    {
        handled = this.graph_submit_sifc(channel, state, method, data);
    }
    else if(nv20_is_texupload_class(class_id))
    {
        handled = this.graph_submit_texupload(channel, state, method, data);
    }
    else if(nv20_is_m2mf_class(class_id))
    {
        handled = this.graph_submit_m2mf(channel, state, method, data);
    }
    else if(nv20_is_swizzled_surface_class(class_id))
    {
        handled = this.graph_submit_swizzled_surface(channel, state, method, data);
    }
    else if(class_id === NV20_CLASS_ROP)
    {
        handled = this.graph_submit_rop(state, method, data);
    }
    else if(nv20_is_pattern_class(class_id))
    {
        handled = this.graph_submit_pattern(state, method, data);
    }
    else if(class_id === NV20_CLASS_CHROMA)
    {
        handled = this.graph_submit_chroma(state, method, data);
    }
    else if(class_id === NV20_CLASS_BETA)
    {
        handled = this.graph_submit_beta(state, method, data);
    }
    else if(nv20_is_sifm_class(class_id))
    {
        handled = this.graph_submit_sifm(channel, state, method, data);
    }
    else if(nv20_is_d3d_class(class_id))
    {
        handled = this.graph_submit_d3d(channel, state, method, data);
    }

    if(!handled && nv20_is_d3d_like_method(method))
    {
        handled = this.graph_submit_d3d(channel, state, method, data);
    }

    this.graph_note_method(state, method, data, handled);

    if(handled)
    {
        this.graph_ctx_user = channel.id << 24;
        this.graph_ctx_control &= ~0x00000100;
        this.graph_status = 0;
        this.graph_complete_notify(channel, subchannel, state, method, data);
    }

    return handled;
};

NV20GeForce.prototype.fifo_submit_method = function(chid, subchannel, method, data, source)
{
    const channel = this.fifo_channel(chid);
    subchannel &= 7;
    method = method & 0x1FFC;
    data >>>= 0;

    this.graph_ctx_user = channel.id << 24;
    this.graph_ctx_control &= ~0x00000100;
    this.graph_status = 0;

    const index = this.fifo_cache1_put >>> 2 & (NV20_FIFO_CACHE_RING_ENTRY_COUNT - 1);
    this.fifo_cache_method[index] = method | subchannel << 13;
    this.fifo_cache_data[index] = data;
    this.fifo_cache1_put = this.fifo_cache1_put + 4 & NV20_FIFO_CACHE_GET_MASK;
    this.fifo_get = this.fifo_cache1_put;
    this.fifo_pull0 &= ~0x100;

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
        this.graph_get_object_state(channel, channel.subchannels[subchannel]);
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

        return true;
    }

    if(this.graph_submit_method(channel, subchannel, method, data))
    {
        return true;
    }

    this["graph_unhandled_method_count"]++;
    this["graph_last_unhandled_method"] = {
        "channel": channel.id,
        "subchannel": subchannel,
        "method": method >>> 0,
        "data": data >>> 0,
        "source": source || "method",
    };
    this.graph_log_unhandled_method(channel, subchannel, method, data, source || "method");
    this.fifo_log_method(channel, subchannel, method, data, source || "method");
    return false;
};

NV20GeForce.prototype.fifo_dma_has_work = function(channel)
{
    return !!channel.pending_count || channel.dma_get !== channel.dma_put;
};

NV20GeForce.prototype.fifo_schedule_dma_kick = function(channel, source)
{
    if(channel.dma_kick_scheduled)
    {
        return;
    }

    channel.dma_kick_scheduled = true;

    const run = () => {
        channel.dma_kick_scheduled = false;
        this.fifo_dma_kick(channel, "async-" + (source || "dma"),
                           NV20_FIFO_DMA_ASYNC_KICK_LIMIT);
    };

    if(typeof setTimeout !== "undefined")
    {
        setTimeout(run, 0);
    }
    else if(typeof requestAnimationFrame !== "undefined")
    {
        requestAnimationFrame(run);
    }
};

NV20GeForce.prototype.fifo_dma_kick = function(channel, source, limit)
{
    if(channel.processing)
    {
        return;
    }

    if(!channel.context_loaded)
    {
        this.fifo_load_channel_context(channel);
    }

    channel.processing = true;

    var budget = limit || NV20_FIFO_DMA_KICK_LIMIT;
    var exhausted = false;

    while(budget-- > 0)
    {
        if(this.fifo_should_wait())
        {
            break;
        }

        if(!this.fifo_dma_object(channel))
        {
            if(this.fifo_recover_dma_instance(channel, source))
            {
                continue;
            }

            if(channel.dma_get !== channel.dma_put || channel.pending_count)
            {
                if(this.fifo_trace)
                {
                    dbg_log(this.name + " pfifo dma channel idle without instance channel=" + channel.id +
                            " get=" + h(channel.dma_get >>> 0, 8) +
                            " put=" + h(channel.dma_put >>> 0, 8) +
                            " source=" + source, LOG_PCI);
                }

                channel.dma_get = channel.dma_put >>> 0;
                channel.pending_count = 0;
                channel.dma_dcount = 0;
                channel.dma_state = 0x80000000;
            }

            break;
        }

        if(channel.pending_count)
        {
            if(channel.dma_get === channel.dma_put)
            {
                break;
            }

            const data = this.fifo_dma_read32(channel, channel.dma_get, source);

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

            if(this.fifo_should_wait())
            {
                break;
            }

            continue;
        }

        if(channel.dma_get === channel.dma_put)
        {
            break;
        }

        const header = this.fifo_dma_read32(channel, channel.dma_get, source);

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
            channel.dma_get = header & 0x1FFFFFFF;
            continue;
        }

        if((header & 3) === 1)
        {
            channel.dma_get = header & 0xFFFFFFFC;
            continue;
        }

        if((header & 3) === 2)
        {
            channel.dma_subroutine = channel.dma_get;
            channel.dma_subroutine_active = true;
            channel.dma_get = header & 0xFFFFFFFC;
            continue;
        }

        if((header & 0xA0030003) === 0)
        {
            channel.pending_non_increasing = !!(header & 0x40000000);
            channel.pending_count = header >>> 18 & 0x7FF;
            channel.pending_subchannel = header >>> 13 & 7;
            channel.pending_method = header & 0x1FFC;
            channel.dma_dcount = channel.pending_count;
            continue;
        }

const bad_get = channel.dma_get - 4 >>> 0;
const bad_object = this.fifo_dma_object(channel);
const bad_translated = bad_object ? this.dma_translate(bad_object, bad_get) : null;

if(bad_object && bad_translated)
{
    const sys_value = this.fifo_read_system32(bad_translated.address);
    const vram_value = this.vram_read32(bad_translated.address % this.vram_size);

    dbg_log(this.name + " pfifo invalid header debug" +
            " ch=" + channel.id +
            " header=" + h(header >>> 0, 8) +
            " get=" + h(bad_get >>> 0, 8) +
            " put=" + h(channel.dma_put >>> 0, 8) +
            " instance=" + h(bad_object.instance, 4) +
            " flags=" + h(bad_object.flags >>> 0, 8) +
            " base_raw=" + h(bad_object.base_raw >>> 0, 8) +
            " base=" + h(bad_object.base >>> 0, 8) +
            " target=" + h(bad_object.target >>> 0, 1) +
            " physical=" + (bad_object.physical ? 1 : 0) +
            " linear=" + (bad_object.linear ? 1 : 0) +
            " translated=" + h(bad_translated.address >>> 0, 8) +
            " sys=" + (sys_value === null ? "null" : h(sys_value >>> 0, 8)) +
            " vram=" + h(vram_value >>> 0, 8),
            LOG_PCI);
}

        this.fifo_cache_error = header >>> 0;

        this.log_missing_command("pfifo-dma-header",
            "ch" + channel.id + ":header" + h(header >>> 0, 8),
            "channel=" + channel.id +
            " header=" + h(header >>> 0, 8) +
            " bad_get=" + h(bad_get >>> 0, 8) +
            " next_get=" + h(channel.dma_get >>> 0, 8) +
            " put=" + h(channel.dma_put >>> 0, 8) +
            " source=" + (source || "dma") +
            " aborted=1");

        // Stop on invalid pushbuffer headers instead of scanning a large
        // 0xFFFFFFFF area on every user-put and stalling the main thread.
        channel.dma_get = channel.dma_put >>> 0;
        channel.pending_count = 0;
        channel.dma_dcount = 0;
        channel.dma_state = 0x80000000;

        this.fifo_sync_cache1_from_channel(channel);
        this.fifo_update_dma_push_state();

        break;
    }

    exhausted = budget <= 0 && !this.fifo_should_wait() && this.fifo_dma_has_work(channel);

    channel.processing = false;
    channel.dma_dcount = channel.pending_count >>> 0;
    channel.dma_state = channel.pending_count ? 0 : 0x80000000;
    this.fifo_sync_cache1_from_channel(channel);
    this.fifo_save_channel_context(channel);
    this.fifo_update_dma_push_state();

    if(exhausted)
    {
        this.fifo_schedule_dma_kick(channel, source);
    }
};

NV20GeForce.prototype.fifo_dma_user_read32 = function(offset)
{
    const rel = offset - NV20_FIFO_DMA_USER_BASE >>> 0;
    const channel = this.fifo_channel(rel / NV20_FIFO_DMA_USER_CHANNEL_STRIDE | 0);
    const reg = rel & (NV20_FIFO_DMA_USER_CHANNEL_STRIDE - 1);

    if((reg & 0x1FFC) === 0x10)
    {
        return 0xFFFF;
    }

    return this.fifo_dma_reg_read32(channel, reg);
};

NV20GeForce.prototype.fifo_dma_user_write32 = function(offset, value)
{
    const rel = offset - NV20_FIFO_DMA_USER_BASE >>> 0;
    const channel = this.fifo_channel(rel / NV20_FIFO_DMA_USER_CHANNEL_STRIDE | 0);
    const reg = rel & (NV20_FIFO_DMA_USER_CHANNEL_STRIDE - 1);

    return this.fifo_dma_reg_write32(channel, reg, value) || true;
};

NV20GeForce.prototype.fifo_pio_user_read32 = function(offset)
{
    const rel = offset - NV20_FIFO_USER_BASE >>> 0;
    const chid = rel / NV20_FIFO_USER_CHANNEL_STRIDE | 0;
    const channel_offset = rel & (NV20_FIFO_USER_CHANNEL_STRIDE - 1);
    const channel = this.fifo_channel(chid);

    if(this.fifo_is_dma_channel(chid) && (channel_offset & 0x1FFC) === 0x10)
    {
        return 0xFFFF;
    }

    if(this.fifo_is_dma_channel(chid) && this.fifo_is_dma_user_reg(channel_offset))
    {
        return this.fifo_dma_reg_read32(channel, channel_offset);
    }

    return 0xFFFFFFFF;
};

NV20GeForce.prototype.fifo_pio_user_write32 = function(offset, value)
{
    const rel = offset - NV20_FIFO_USER_BASE >>> 0;
    const chid = rel / NV20_FIFO_USER_CHANNEL_STRIDE | 0;
    const channel_offset = rel & (NV20_FIFO_USER_CHANNEL_STRIDE - 1);
    const subchannel = channel_offset / NV20_FIFO_USER_SUBCHANNEL_STRIDE | 0;
    const method = channel_offset & (NV20_FIFO_USER_SUBCHANNEL_STRIDE - 1) & 0x1FFC;

    if(this.fifo_is_dma_channel(chid) && this.fifo_is_dma_user_reg(channel_offset))
    {
        return this.fifo_dma_reg_write32(this.fifo_channel(chid), channel_offset, value);
    }

    this.fifo_set_active_channel(chid);
    this.fifo_submit_method(chid, subchannel, method, value, "pio");
    return true;
};

NV20GeForce.prototype.vram_mark_write = function(offset, width, value)
{
    this.vram_write_count++;

    this.vram_sync_fast_range(offset, width);

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

        this.render_mark_dirty_rows(dirty_start, dirty_end);
        this.activate_rendering();
        this.schedule_render();
    }

    if(this.hw_cursor_enabled && this.hw_cursor_vram)
    {
        const cursor_bytes = this.hw_cursor_size * this.hw_cursor_size *
            (this.hw_cursor_bpp32 ? 4 : 2);
        const cursor_start = this.hw_cursor_offset >>> 0;
        const cursor_end = cursor_start + cursor_bytes >>> 0;

        if(cursor_start < this.vram_size &&
            end > cursor_start &&
            offset < Math.min(this.vram_size, cursor_end))
        {
            this.hw_cursor_mark_dirty(this.hw_cursor_x, this.hw_cursor_y, this.hw_cursor_size);
        }
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
    const render_format = nv20_render_format_from_bpp(bpp, this.render_format);
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
        this.render_format !== render_format ||
        this.render_stride !== stride ||
        this.render_offset !== offset;

    if(!changed)
    {
        return true;
    }

    if(this.vram_fast_memory)
    {
        this.vram_fast_dirty = false;
        this.vram_fast_dirty_min = this.vram_fast_size;
        this.vram_fast_dirty_max = 0;
        this.vram_fast_sync_key = "";
        this.vram_fast_discard_svga_dirty = true;
    }

    this.render_width = width;
    this.render_height = height;
    this.render_bpp = bpp;
    this.render_format = render_format;
    this.render_stride = stride;
    this.render_offset = offset;
    this.render_frame_size = frame_size;
    this.render_dirty_min = offset;
    this.render_dirty_max = offset + frame_size;
    this.render_dirty_row_min = this.render_height;
    this.render_dirty_row_max = 0;
    this.render_dirty_rows = null;
    this.render_initialized = false;
    this.render_buffer = null;
    this.render_image_data = null;
    this.render_source = source || "registers";

    this["debug_render_mode"] = {
        "width": width,
        "height": height,
        "bpp": bpp,
        "stride": stride,
        "offset": offset,
        "source": this.render_source,
        "surface": this.render_surface_inferred,
        "tile": !!(this.fb_tile0_flags & 1),
        "tile_pitch": this.fb_tile0_pitch,
    };

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

NV20GeForce.prototype.update_render_mode_from_surface = function(surface, source, width_hint, height_hint)
{
    if(!this.render_auto_detect || !surface)
    {
        return false;
    }

    const format = surface.format & 0xFF;

    if(format < 1 || format > 0x0B)
    {
        return false;
    }

    if(!surface.surface_format_set && format === nv20_surface_format_from_bpp(this.render_bpp))
    {
        return false;
    }

    const bpp = nv20_surface_bpp_from_format(format, 0);
    const bytes_per_pixel = nv20_render_bytes_per_pixel(bpp);

    if(!nv20_sane_render_bpp(bpp) || !bytes_per_pixel)
    {
        return false;
    }

    const crtc_mode = this.crtc_render_mode();

    if(crtc_mode && crtc_mode.bpp !== bpp)
    {
        return false;
    }

    if(surface.surface_dst_offset_set === false)
    {
        return false;
    }

    var offset = surface.dst_offset >>> 0;
    const tile_active = !!((this.fb_tile0_flags & 1) &&
        this.fb_tile0_pitch &&
        offset === 0);
    var stride = tile_active ? this.fb_tile0_pitch :
        (surface.surface_pitch_set && surface.dst_pitch ||
         this.graph_pitch0 || this.fb_tile0_pitch || 0) >>> 0;

    if(!stride || offset >= this.vram_size)
    {
        return false;
    }

    if(stride & 3)
    {
        stride = stride + 3 & ~3;
    }

    const max_width = stride / bytes_per_pixel | 0;

    if(max_width < NV20_MIN_RENDER_WIDTH)
    {
        return false;
    }

    // Keep automatic surface scanout on the front buffer. Offscreen surfaces
    // are common during XP startup and should not move the visible display.
    if(offset && offset !== this.render_offset && offset !== this.crtc_start)
    {
        return false;
    }

    const tile_limit = this.fb_tile0_limit >>> 0;
    const tile_height = tile_limit > offset && this.fb_tile0_pitch === stride ?
        ((tile_limit - offset + 1) / stride | 0) : 0;
    const vram_height = (this.vram_size - offset) / stride | 0;
    const max_height = Math.max(1, tile_height || vram_height);
    var width = 0;
    var height = 0;

    width_hint = width_hint >>> 0;
    height_hint = height_hint >>> 0;

    for(var i = 0; i < NV20_RENDER_COMMON_MODES.length; i++)
    {
        const mode = NV20_RENDER_COMMON_MODES[i];

        if(mode[0] <= max_width && mode[1] <= max_height)
        {
            width = mode[0];
            height = mode[1];
            break;
        }
    }

    if(!width &&
        width_hint >= NV20_MIN_RENDER_WIDTH &&
        width_hint <= max_width &&
        height_hint >= NV20_MIN_RENDER_HEIGHT &&
        height_hint <= max_height)
    {
        width = width_hint;
        height = height_hint;
    }

    if(!width || !height)
    {
        if(this.render_width > max_width || this.render_height > max_height)
        {
            return false;
        }

        width = this.render_width;
        height = this.render_height;
    }

    if(crtc_mode)
    {
        if(offset !== crtc_mode.offset)
        {
            return false;
        }

        width = crtc_mode.width;
        height = crtc_mode.height;
        stride = crtc_mode.stride;
    }

    this.render_surface_inferred = true;
    this.render_surface_pitch = stride;
    this.render_surface_offset = offset;
    this.render_surface_bpp = bpp;
    return this.set_render_mode(width, height, bpp, stride, offset, source || "surface2d");
};

function nv20_crtc_register_affects_render_mode(index)
{
    switch(index & 0xFF)
    {
        case 0x01:
        case 0x07:
        case 0x12:
        case 0x13:
        case 0x19:
        case 0x25:
        case 0x28:
        case 0x2D:
        case 0x41:
            return true;
        default:
            return false;
    }
}

function nv20_render_modes_equal(a, b)
{
    return !!a && !!b &&
        a.width === b.width &&
        a.height === b.height &&
        a.bpp === b.bpp &&
        a.stride === b.stride &&
        a.offset === b.offset;
}

NV20GeForce.prototype.crtc_render_mode = function()
{
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
        return null;
    }

    const bpp = pixel_mode === 1 ? 8 :
        pixel_mode === 2 ? 16 :
        pixel_mode === 3 ? 32 :
        this.render_bpp;
    const bytes_per_pixel = nv20_render_bytes_per_pixel(bpp);
    var stride = row_offset * 8;

    if(!bytes_per_pixel)
    {
        return null;
    }

    if(stride < width * bytes_per_pixel)
    {
        stride = width * bytes_per_pixel;
    }

    return {
        width: width,
        height: height,
        bpp: bpp,
        stride: stride,
        offset: this.crtc_start,
    };
};

NV20GeForce.prototype.update_render_mode_from_d3d_surface = function(state, source)
{
    if(!this.render_auto_detect || !state)
    {
        return false;
    }

    const surface_format = nv20_surface_format_from_d3d_format(state.class_id, state.d3d_surface_format);
    const bpp = nv20_d3d_bpp_from_format(state.class_id, state.d3d_surface_format, 0);
    const pitch = (state.d3d_surface_pitch & 0xFFFF) || state.d3d_surface_pitch;

    if(!surface_format || !nv20_sane_render_bpp(bpp) || !pitch)
    {
        return false;
    }

    return this.update_render_mode_from_surface({
        class_id: state.class_id,
        class_name: (state.class_name || "d3d") + "-surface",
        format: surface_format,
        src_pitch: pitch,
        dst_pitch: pitch,
        src_offset: state.d3d_surface_color_offset >>> 0,
        dst_offset: state.d3d_surface_color_offset >>> 0,
        surface_format_set: true,
        surface_pitch_set: true,
        surface_src_offset_set: true,
        surface_dst_offset_set: true,
        src_dma: state.d3d_color_dma || null,
        dst_dma: state.d3d_color_dma || null,
    }, source || "d3d-surface", this.render_width, this.render_height);
};

NV20GeForce.prototype.update_render_mode_from_crtc = function(source)
{
    if(!this.render_auto_detect)
    {
        return;
    }

    const mode = this.crtc_render_mode();

    if(!mode)
    {
        return;
    }

    if(this.dispi_pending_disable)
    {
        return;
    }

    const dispi_mode = this.dispi_render_mode();

    if(dispi_mode && !nv20_render_modes_equal(mode, dispi_mode))
    {
        return;
    }

    this.render_surface_inferred = false;
    this.set_render_mode(mode.width, mode.height, mode.bpp, mode.stride, mode.offset, source);
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

NV20GeForce.prototype.sync_fast_lfb_vga = function()
{
    const vga = this.cpu && this.cpu.devices && this.cpu.devices.vga;

    if(!this.vram_fast_memory ||
        !vga ||
        this.render_offset + this.render_frame_size > this.vram_fast_size ||
        !vga.screen_fill_external_lfb)
    {
        return false;
    }

    const bytes_per_pixel = nv20_render_bytes_per_pixel(this.render_bpp);

    if(!bytes_per_pixel)
    {
        return false;
    }

    const offset_pixels = this.render_offset / bytes_per_pixel | 0;
    const key = this.render_width + "x" + this.render_height + "x" +
        this.render_bpp + ":" + this.render_format +
        " stride=" + this.render_stride +
        "@" + offset_pixels;

    this.vram_fast_sync_config = {
        width: this.render_width,
        height: this.render_height,
        bpp: this.render_bpp,
        stride: this.render_stride,
        offset: this.render_offset,
        format: this.render_format,
        dirty_min: this.vram_fast_dirty_min,
        dirty_max: this.vram_fast_dirty_max,
        discard_svga_dirty: this.vram_fast_discard_svga_dirty,
    };

    if(this.vram_fast_sync_key !== key)
    {
        this.vram_fast_sync_key = key;
    }

    return true;
};

NV20GeForce.prototype.screen_fill_buffer = function()
{
    if(!this.render_active)
    {
        return false;
    }

    if(this.sync_fast_lfb_vga())
    {
        if(this.cpu.devices.vga.screen_fill_external_lfb(this.vram_fast_sync_config))
        {
            this.vram_fast_dirty = false;
            this.vram_fast_dirty_min = this.vram_fast_size;
            this.vram_fast_dirty_max = 0;
            this.vram_fast_discard_svga_dirty = false;
            this.render_clear_dirty();
            return true;
        }
    }

    if(this.render_dirty_min >= this.render_dirty_max)
    {
        return true;
    }

    if(!this.ensure_render_buffer())
    {
        this.render_clear_dirty();
        return true;
    }

    const stride = this.render_stride;
    var layers = [];
    var min_y;
    var max_y;

    if(this.render_dirty_rows && this.render_dirty_row_min < this.render_dirty_row_max)
    {
        const dirty_row_min = this.render_dirty_row_min;
        const dirty_row_max = Math.min(this.render_height, this.render_dirty_row_max);
        var y = dirty_row_min;
        var dirty_rows = 0;
        var dirty_layers = 0;
        var in_dirty_layer = false;

        while(y < dirty_row_max)
        {
            if(this.render_dirty_rows[y])
            {
                dirty_rows++;

                if(!in_dirty_layer)
                {
                    dirty_layers++;
                    in_dirty_layer = true;
                }
            }
            else
            {
                in_dirty_layer = false;
            }

            y++;
        }

        if(dirty_layers > NV20_RENDER_DIRTY_LAYER_LIMIT)
        {
            min_y = dirty_row_min;
            max_y = dirty_row_max;

            if(dirty_rows && min_y < max_y)
            {
                this.render_rows(min_y, max_y);
                layers.push({
                    image_data: this.render_image_data,
                    screen_x: 0,
                    screen_y: min_y,
                    buffer_x: 0,
                    buffer_y: min_y,
                    buffer_width: this.render_width,
                    buffer_height: max_y - min_y,
                });
            }
        }
        else
        {
            y = dirty_row_min;

            while(y < dirty_row_max)
            {
                while(y < dirty_row_max && !this.render_dirty_rows[y])
                {
                    y++;
                }

                min_y = y;

                while(y < dirty_row_max && this.render_dirty_rows[y])
                {
                    this.render_dirty_rows[y] = 0;
                    y++;
                }

                max_y = y;

                if(min_y < max_y)
                {
                    this.render_rows(min_y, max_y);
                    layers.push({
                        image_data: this.render_image_data,
                        screen_x: 0,
                        screen_y: min_y,
                        buffer_x: 0,
                        buffer_y: min_y,
                        buffer_width: this.render_width,
                        buffer_height: max_y - min_y,
                    });
                }
            }
        }
    }
    else
    {
        const dirty_min = Math.max(0, this.render_dirty_min - this.render_offset);
        const dirty_max = Math.min(this.render_frame_size, this.render_dirty_max - this.render_offset);
        min_y = Math.max(0, Math.min(this.render_height, dirty_min / stride | 0));
        max_y = Math.max(min_y, Math.min(this.render_height, (dirty_max + stride - 1) / stride | 0));

        if(min_y < max_y)
        {
            this.render_rows(min_y, max_y);
            layers.push({
                image_data: this.render_image_data,
                screen_x: 0,
                screen_y: min_y,
                buffer_x: 0,
                buffer_y: min_y,
                buffer_width: this.render_width,
                buffer_height: max_y - min_y,
            });
        }
    }

    if(layers.length)
    {
        this.screen.update_buffer(layers);
        this.render_update_count++;

        if(this.render_update_count === 1)
        {
            dbg_log(this.name + " render bridge frame update y=" + layers[0].screen_y +
                    " rows=" + layers[0].buffer_height +
                    " layers=" + layers.length, LOG_PCI);
        }
    }

    this.render_clear_dirty();
    return true;
};

NV20GeForce.prototype.render_rows = function(min_y, max_y)
{
    const src = this.vram;
    const dst = this.render_buffer;

    if(this.render_bpp === 8)
    {
        const vga = this.cpu && this.cpu.devices && this.cpu.devices.vga;
        const palette = vga && vga.vga256_palette;

        for(var y = min_y; y < max_y; y++)
        {
            var src_i = this.render_offset + y * this.render_stride;
            var dst_i = y * this.render_width * 4;

            for(var x = 0; x < this.render_width; x++, src_i++, dst_i += 4)
            {
                const index = src[src_i];
                const color = palette ? palette[index] >>> 0 : index * 0x010101;
                dst[dst_i] = color >>> 16 & 0xFF;
                dst[dst_i + 1] = color >>> 8 & 0xFF;
                dst[dst_i + 2] = color & 0xFF;
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

    this.render_overlay_hw_cursor(min_y, max_y);
};

NV20GeForce.prototype.register_rma_io = function()
{
    const io = this.cpu.io;

    io.register_read(0x3D0, this, this.rma_read8_d0, this.rma_read16_d0, this.rma_read32_d0);
    io.register_read(0x3D1, this, this.rma_read8_d1);
    io.register_read(0x3D2, this, this.rma_read8_d2, this.rma_read16_d2);
    io.register_read(0x3D3, this, this.rma_read8_d3);

    io.register_write(0x3D0, this, this.rma_write8_d0, this.rma_write16_d0, this.rma_write32_d0);
    io.register_write(0x3D1, this, this.rma_write8_d1);
    io.register_write(0x3D2, this, this.rma_write8_d2, this.rma_write16_d2);
    io.register_write(0x3D3, this, this.rma_write8_d3);
};

NV20GeForce.prototype.register_legacy_vga_io = function()
{
    const io = this.cpu.io;
    const write8 = port => value => this.vga_port_write8(port, value);
    const write16 = port => value => this.vga_port_write16(port, value);

    io.register_read(0x3C0, this, this.legacy_vga_port_read8, this.legacy_vga_port_read16);
    io.register_read(0x3C1, this, this.legacy_vga_port_read8, this.legacy_vga_port_read16);
    io.register_read(0x3C2, this, this.legacy_vga_port_read8, this.legacy_vga_port_read16);
    io.register_read(0x3C3, this, this.legacy_vga_port_read8, this.legacy_vga_port_read16);
    io.register_read(0x3C4, this, this.legacy_vga_port_read8, this.legacy_vga_port_read16);
    io.register_read(0x3C5, this, this.legacy_vga_port_read8, this.legacy_vga_port_read16);
    io.register_read(0x3C6, this, this.legacy_vga_port_read8, this.legacy_vga_port_read16);
    io.register_read(0x3C7, this, this.legacy_vga_port_read8, this.legacy_vga_port_read16);
    io.register_read(0x3C8, this, this.legacy_vga_port_read8, this.legacy_vga_port_read16);
    io.register_read(0x3C9, this, this.legacy_vga_port_read8, this.legacy_vga_port_read16);
    io.register_read(0x3CA, this, this.legacy_vga_port_read8, this.legacy_vga_port_read16);
    io.register_read(0x3CC, this, this.legacy_vga_port_read8, this.legacy_vga_port_read16);
    io.register_read(0x3CE, this, this.legacy_vga_port_read8, this.legacy_vga_port_read16);
    io.register_read(0x3CF, this, this.legacy_vga_port_read8, this.legacy_vga_port_read16);
    io.register_read(0x3BA, this, this.legacy_vga_port_read8, this.legacy_vga_port_read16);
    io.register_read(0x3DA, this, this.legacy_vga_port_read8, this.legacy_vga_port_read16);

    io.register_write(0x3C0, this, write8(0x3C0), write16(0x3C0));
    io.register_write(0x3C2, this, write8(0x3C2), write16(0x3C2));
    io.register_write(0x3C3, this, write8(0x3C3), write16(0x3C3));
    io.register_write(0x3C4, this, write8(0x3C4), write16(0x3C4));
    io.register_write(0x3C5, this, write8(0x3C5), write16(0x3C5));
    io.register_write(0x3C6, this, write8(0x3C6), write16(0x3C6));
    io.register_write(0x3C7, this, write8(0x3C7), write16(0x3C7));
    io.register_write(0x3C8, this, write8(0x3C8), write16(0x3C8));
    io.register_write(0x3C9, this, write8(0x3C9), write16(0x3C9));
    io.register_write(0x3CE, this, write8(0x3CE), write16(0x3CE));
    io.register_write(0x3CF, this, write8(0x3CF), write16(0x3CF));
    io.register_write(0x3D8, this, write8(0x3D8), write16(0x3D8));
    io.register_write(0x3DA, this, write8(0x3DA), write16(0x3DA));

    io.register_read(0x3B4, this, this.legacy_vga_read8_3b4, this.legacy_vga_read16_3b4);
    io.register_read(0x3B5, this, this.legacy_vga_read8_3b5, this.legacy_vga_read16_3b5);
    io.register_read(0x3D4, this, this.legacy_vga_read8_3d4, this.legacy_vga_read16_3d4);
    io.register_read(0x3D5, this, this.legacy_vga_read8_3d5, this.legacy_vga_read16_3d5);

    io.register_write(0x3B4, this, this.legacy_vga_write8_3b4, this.legacy_vga_write16_3b4);
    io.register_write(0x3B5, this, this.legacy_vga_write8_3b5, this.legacy_vga_write16_3b5);
    io.register_write(0x3D4, this, this.legacy_vga_write8_3d4, this.legacy_vga_write16_3d4);
    io.register_write(0x3D5, this, this.legacy_vga_write8_3d5, this.legacy_vga_write16_3d5);

    io.register_read(0x1CE, this, this.dispi_index_read8, this.dispi_index_read16);
    io.register_write(0x1CE, this, this.dispi_index_write8, this.dispi_index_write16);
    io.register_read(0x1CF, this, this.dispi_data_read8, this.dispi_data_read16);
    io.register_write(0x1CF, this, this.dispi_data_write8, this.dispi_data_write16);

    dbg_log(this.name + " legacy VGA CRTC I/O owner installed", LOG_PCI);
};

NV20GeForce.prototype.register_legacy_vga_memory = function()
{
    this.cpu.io.mmap_register(NV20_LEGACY_VGA_MEM_BASE, NV20_LEGACY_VGA_MEM_SIZE,
        addr => this.legacy_vga_memory_read8(addr),
        (addr, value) => this.legacy_vga_memory_write8(addr, value));

    dbg_log(this.name + " legacy VGA memory owner installed", LOG_PCI);
};

NV20GeForce.prototype.rma_read_target32 = function(for_write)
{
    const address = this.rma_addr >>> 0;
    var offset = for_write ? address & ~3 : address;
    const vram = !!(offset & 0x80000000);

    if(vram)
    {
        offset = offset & ~0x80000000;
        return offset < this.vram_size ? this.vram_read32(offset) : 0xFFFFFFFF;
    }

    if(offset < NV20_MMIO_SIZE)
    {
        const result = this.register_read32(offset & ~3).value;
        this.mmio_note_hot_poll(offset & ~3, result, 32);
        return result;
    }

    return 0xFFFFFFFF;
};

NV20GeForce.prototype.rma_write_target32 = function(value)
{
    const address = this.rma_addr >>> 0;
    var offset = address & ~3;
    const vram = !!(offset & 0x80000000);

    value >>>= 0;

    if(vram)
    {
        offset = offset & ~0x80000000;
        if(offset < this.vram_size)
        {
            this.vram_write32(offset, value);
        }
    }
    else if(offset < NV20_MMIO_SIZE)
    {
        this.register_write32(offset, value);
    }
};

NV20GeForce.prototype.rma_read16 = function(port)
{
    const crtc38 = this.prmcio_crtc_regs[0x38];

    if(!(crtc38 & 1))
    {
        return 0;
    }

    const rma_index = crtc38 >> 1;
    const high = (port & 2) !== 0;

    if(rma_index === 1)
    {
        return high ? this.rma_addr >>> 16 & 0xFFFF : this.rma_addr & 0xFFFF;
    }

    if(rma_index === 2)
    {
        const value = this.rma_read_target32(false);
        return high ? value >>> 16 & 0xFFFF : value & 0xFFFF;
    }

    return 0;
};

NV20GeForce.prototype.rma_read32 = function()
{
    const crtc38 = this.prmcio_crtc_regs[0x38];

    if(!(crtc38 & 1))
    {
        return 0;
    }

    const rma_index = crtc38 >> 1;

    if(rma_index === 1)
    {
        return this.rma_addr | 0;
    }

    if(rma_index === 2)
    {
        return this.rma_read_target32(false) | 0;
    }

    return 0;
};

NV20GeForce.prototype.rma_write16 = function(port, value)
{
    const crtc38 = this.prmcio_crtc_regs[0x38];

    value &= 0xFFFF;

    if(!(crtc38 & 1))
    {
        return;
    }

    const rma_index = crtc38 >> 1;
    const high = (port & 2) !== 0;

    if(rma_index === 1)
    {
        if(high)
        {
            this.rma_addr = (this.rma_addr & 0x0000FFFF | value << 16) >>> 0;
        }
        else
        {
            this.rma_addr = (this.rma_addr & 0xFFFF0000 | value) >>> 0;
        }
    }
    else if(rma_index === 3)
    {
        var value32 = this.rma_read_target32(true);
        value32 = high ? value32 & 0x0000FFFF | value << 16 : value32 & 0xFFFF0000 | value;
        this.rma_write_target32(value32);
    }
};

NV20GeForce.prototype.rma_write32 = function(value)
{
    const crtc38 = this.prmcio_crtc_regs[0x38];

    value >>>= 0;

    if(!(crtc38 & 1))
    {
        return;
    }

    const rma_index = crtc38 >> 1;

    if(rma_index === 1)
    {
        this.rma_addr = value;
    }
    else if(rma_index === 3)
    {
        this.rma_write_target32(value);
    }
};

NV20GeForce.prototype.rma_read8 = function(port)
{
    const value = this.rma_read16(port & ~1);
    return port & 1 ? value >> 8 & 0xFF : value & 0xFF;
};

NV20GeForce.prototype.rma_write8 = function(port, value)
{
    const base = port & ~1;
    const shift = port & 1 ? 8 : 0;
    const old_value = this.rma_read16(base);
    this.rma_write16(base, old_value & ~(0xFF << shift) | (value & 0xFF) << shift);
};

NV20GeForce.prototype.rma_read8_d0 = function(port) { return this.rma_read8(port); };
NV20GeForce.prototype.rma_read8_d1 = function(port) { return this.rma_read8(port); };
NV20GeForce.prototype.rma_read8_d2 = function(port) { return this.rma_read8(port); };
NV20GeForce.prototype.rma_read8_d3 = function(port) { return this.rma_read8(port); };
NV20GeForce.prototype.rma_read16_d0 = function() { return this.rma_read16(0x3D0); };
NV20GeForce.prototype.rma_read16_d2 = function() { return this.rma_read16(0x3D2); };
NV20GeForce.prototype.rma_read32_d0 = function() { return this.rma_read32(); };
NV20GeForce.prototype.rma_write8_d0 = function(value) { this.rma_write8(0x3D0, value); };
NV20GeForce.prototype.rma_write8_d1 = function(value) { this.rma_write8(0x3D1, value); };
NV20GeForce.prototype.rma_write8_d2 = function(value) { this.rma_write8(0x3D2, value); };
NV20GeForce.prototype.rma_write8_d3 = function(value) { this.rma_write8(0x3D3, value); };
NV20GeForce.prototype.rma_write16_d0 = function(value) { this.rma_write16(0x3D0, value); };
NV20GeForce.prototype.rma_write16_d2 = function(value) { this.rma_write16(0x3D2, value); };
NV20GeForce.prototype.rma_write32_d0 = function(value) { this.rma_write32(value); };

NV20GeForce.prototype.prmcio_set_crtc_index = function(value)
{
    this.prmcio_crtc_index = value & 0xFF;
    return true;
};

NV20GeForce.prototype.prmcio_write_crtc_data = function(value)
{
    value &= 0xFF;

    const index = this.prmcio_crtc_index & 0xFF;

    if(this.dispi_delegated_crtc_writes)
    {
        this.dispi_clear_delegated_crtc();
    }

    if(index === 0x1C && !(this.prmcio_crtc_regs[index] & 0x80) && (value & 0x80))
    {
        this.crtc_intr_en = 0;
        this.update_irq_level();
    }
    else if(index === 0x58)
    {
        return true;
    }
    else if(index === NV20_CRTC_DDC0_STATUS ||
            index === NV20_CRTC_DDC0_WRITE ||
            index === NV20_CRTC_I2C_READ ||
            index === NV20_CRTC_I2C_WRITE)
    {
        this.prmcio_crtc_regs[index] = value;

        if(index === NV20_CRTC_DDC0_WRITE || index === NV20_CRTC_I2C_WRITE)
        {
            this.prmcio_crtc_regs[NV20_CRTC_DDC0_STATUS] |= NV20_CRTC_DDC_LINES_HIGH;
            this.prmcio_crtc_regs[NV20_CRTC_I2C_READ] |= NV20_CRTC_DDC_LINES_HIGH;
        }

        return true;
    }

    this.prmcio_crtc_regs[index] = value;

    if(nv20_crtc_register_affects_render_mode(index))
    {
        this.update_render_mode_from_crtc("crtc[" + h(index, 2) + "]");
    }

    if(index === 0x2F || index === 0x30 || index === 0x31)
    {
        this.hw_cursor_update(true);
    }

    return true;
};

NV20GeForce.prototype.crtc_next_raster_status = function()
{
    const visible = Math.max(1, this.render_height || this.dispi_height || NV20_DEFAULT_RENDER_HEIGHT);
    const total = visible + Math.max(16, visible >> 4);

    this.crtc_raster_counter = (this.crtc_raster_counter + 1) >>> 0;
    this.crtc_status_read_count = (this.crtc_status_read_count + 1) >>> 0;

    // Bochs advances retrace timing independently of CPU I/O polling. v86 does
    // not have that timer here, so make reads progress through a compact
    // visible/vblank cycle. This prevents XP's miniport from busy-waiting
    // forever on 0x3DA/PCRTC_RASTER while still exposing both phases.
    const phase = this.crtc_status_read_count & (NV20_CRTC_RASTER_POLL_PHASES - 1);
    const vblank = phase < 2;
    const display_enable = phase & 1;
    const visible_line = this.crtc_raster_counter % visible;
    const line = vblank ? visible + phase : visible_line;
    const status = display_enable | (vblank ? 0x08 : 0x00);

    this.legacy_vga_status = status;

    return {
        line: Math.min(line, total - 1),
        status: status,
    };
};

NV20GeForce.prototype.prmcio_read_crtc_data = function()
{
    const index = this.prmcio_crtc_index & 0xFF;

    if(index === NV20_CRTC_DDC0_STATUS || index === NV20_CRTC_I2C_READ)
    {
        // The XP NVIDIA miniport probes monitor DDC through NV CRTC I2C bits.
        // With no emulated monitor bus, report the wire state as idle high.
        // Do not echo the write latch here: the guest polls the line state,
        // not the last drive value.
        return NV20_CRTC_DDC_LINES_HIGH;
    }

    if(index === NV20_CRTC_DDC0_WRITE || index === NV20_CRTC_I2C_WRITE)
    {
        return NV20_CRTC_DDC_WRITE_LINES_HIGH;
    }

    return this.prmcio_crtc_regs[index];
};

NV20GeForce.prototype.reset_legacy_vga_shadow = function()
{
    this.legacy_vga_memory.fill(0);
    this.legacy_vga_attr_regs.fill(0);
    this.legacy_vga_seq_regs.fill(0);
    this.legacy_vga_graphics_regs.fill(0);
    this.legacy_vga_dac_data.fill(0);

    this.legacy_vga_attr_index = 0;
    this.legacy_vga_attr_palette_source = 0x20;
    this.legacy_vga_attr_data_phase = false;
    this.legacy_vga_seq_index = 0;
    this.legacy_vga_graphics_index = 0;
    this.legacy_vga_dac_mask = 0xFF;
    this.legacy_vga_dac_read = 0;
    this.legacy_vga_dac_write = 0;
    this.legacy_vga_misc_output = 0xFF;
    this.legacy_vga_enable = 0;
    this.legacy_vga_status = 0;

    this.legacy_vga_seq_regs[0x02] = 0x0F;
    this.legacy_vga_seq_regs[0x04] = 0x06;
    this.legacy_vga_graphics_regs[0x06] = 0x0C;
};

NV20GeForce.prototype.reset_dispi_shadow = function()
{
    this.dispi_index = 0;
    this.dispi_version = NV20_DISPI_VERSION;
    this.dispi_enable_value = 0;
    this.dispi_width = 640;
    this.dispi_height = 480;
    this.dispi_bpp = 8;
    this.dispi_bank = 0;
    this.dispi_virtual_width = 0;
    this.dispi_virtual_height = 0;
    this.dispi_offset_x = 0;
    this.dispi_offset_y = 0;
    this.dispi_takeover_logged = false;
    this.dispi_pending_disable = false;
    this.dispi_delegated_crtc_writes = 0;
    this.dispi_delegated_crtc_log_count = 0;
    this.dispi_suppressed_crtc_writes = 0;
    this.dispi_suppressed_crtc_log_count = 0;
    this.dispi_ignore_boot_mode_sets = 0;
    this.dispi_ignoring_boot_mode_set = false;
};

NV20GeForce.prototype.geforce_owns_legacy_vga = function()
{
    return this.render_active || this.render_initialized ||
        !!(this.pci_config_space8 && (this.pci_config_space8[0x04] & 0x01));
};

NV20GeForce.prototype.geforce_handles_dispi = function()
{
    // The Bochs DISPI/VBE ports belong to v86's Bochs VGA adapter. Windows can
    // keep using that path during boot even when the GeForce PCI device is
    // present, so swallowing these writes here leaves the visible framebuffer
    // blank. Native GeForce scanout is driven by NV registers/surfaces instead.
    return false;
};

NV20GeForce.prototype.dispi_vga_delegate = function()
{
    return this.cpu && this.cpu.devices && this.cpu.devices.vga || null;
};

NV20GeForce.prototype.dispi_note_delegated_vbe_write = function(index, value)
{
    index &= 0xFFFF;
    value &= 0xFFFF;

    if(index === 4)
    {
        if(value & 1)
        {
            this.dispi_delegated_crtc_writes = 128;
            this.dispi_delegated_crtc_log_count = 0;
            this.dispi_suppressed_crtc_writes = 0;
        }
        else if(this.render_active)
        {
            this.dispi_pending_disable = true;
            this.dispi_delegated_crtc_writes = 0;
            this.dispi_suppressed_crtc_writes = 128;
            this.dispi_suppressed_crtc_log_count = 0;
        }
        else
        {
            this.dispi_delegated_crtc_writes = 128;
            this.dispi_delegated_crtc_log_count = 0;
            this.dispi_suppressed_crtc_writes = 0;
        }
    }
};

NV20GeForce.prototype.dispi_delegated_crtc_active = function()
{
    return this.dispi_delegated_crtc_writes > 0 &&
        this.prmcio_crtc_index <= NV20_VGA_CRTC_MAX;
};

NV20GeForce.prototype.dispi_suppressed_crtc_active = function()
{
    return this.dispi_suppressed_crtc_writes > 0 &&
        this.prmcio_crtc_index <= NV20_VGA_CRTC_MAX;
};

NV20GeForce.prototype.dispi_clear_delegated_crtc = function()
{
    this.dispi_delegated_crtc_writes = 0;
    this.dispi_suppressed_crtc_writes = 0;
    this.dispi_pending_disable = false;
};

NV20GeForce.prototype.dispi_ignore_boot_mode_write = function(index, value)
{
    index &= 0xFFFF;
    value &= 0xFFFF;

    if(this.dispi_ignore_boot_mode_sets <= 0 && !this.dispi_ignoring_boot_mode_set)
    {
        return false;
    }

    if(!this.dispi_ignoring_boot_mode_set)
    {
        if(index !== 4 || value & 1)
        {
            return false;
        }

        this.dispi_ignoring_boot_mode_set = true;
    }

    if(index === 4 && (value & 1))
    {
        this.dispi_ignoring_boot_mode_set = false;
        this.dispi_ignore_boot_mode_sets--;
        this.dispi_pending_disable = false;
        this.dispi_delegated_crtc_writes = 0;
        this.dispi_suppressed_crtc_writes = 128;
        this.dispi_suppressed_crtc_log_count = 0;

        dbg_log(this.name + " ignored initial Bochs VBE mode set", LOG_PCI);
    }

    return true;
};

NV20GeForce.prototype.dispi_report_geforce_mode = function()
{
    const mode = this["debug_render_mode"];

    if(!this.bus ||
        !mode ||
        mode["source"] === "default" ||
        !mode["width"] ||
        !mode["height"] ||
        !mode["bpp"])
    {
        return;
    }

    this.bus.send("screen-set-size", [mode["width"], mode["height"], mode["bpp"]]);
};

NV20GeForce.prototype.dispi_render_mode = function()
{
    if(!(this.dispi_enable_value & 1))
    {
        return null;
    }

    const bpp = nv20_sane_render_bpp(this.dispi_bpp) ? this.dispi_bpp : this.render_bpp;
    const bytes_per_pixel = nv20_render_bytes_per_pixel(bpp);

    if(!bytes_per_pixel)
    {
        return null;
    }

    const width = this.dispi_width || this.render_width;
    const height = this.dispi_height || this.render_height;
    const virtual_width = Math.max(width, this.dispi_virtual_width || 0);

    return {
        width: width,
        height: height,
        bpp: bpp,
        stride: virtual_width * bytes_per_pixel,
        offset: 0,
    };
};

NV20GeForce.prototype.dispi_mirror_crtc_mode = function(mode)
{
    if(!mode)
    {
        return;
    }

    const crtc = this.prmcio_crtc_regs;
    const horizontal_display_chars = Math.max(1, mode.width >>> 3) - 1;
    const vertical_display = Math.max(1, mode.height) - 1;
    const row_offset = Math.max(1, mode.stride >>> 3);
    const pixel_mode =
        mode.bpp === 8 ? 1 :
        mode.bpp === 15 || mode.bpp === 16 ? 2 :
        mode.bpp === 32 ? 3 : 0;

    crtc[0x01] = horizontal_display_chars & 0xFF;
    crtc[0x2D] = crtc[0x2D] & ~0x02 | (horizontal_display_chars & 0x100 ? 0x02 : 0);

    crtc[0x12] = vertical_display & 0xFF;
    crtc[0x07] = crtc[0x07] & ~(0x02 | 0x40) |
        (vertical_display & 0x100 ? 0x02 : 0) |
        (vertical_display & 0x200 ? 0x40 : 0);
    crtc[0x25] = crtc[0x25] & ~0x02 | (vertical_display & 0x400 ? 0x02 : 0);
    crtc[0x41] = crtc[0x41] & ~0x04 | (vertical_display & 0x800 ? 0x04 : 0);

    if(pixel_mode)
    {
        crtc[0x28] = crtc[0x28] & ~0x03 | pixel_mode;
    }

    crtc[0x13] = row_offset & 0xFF;
    crtc[0x19] = crtc[0x19] & ~0xE0 | (row_offset >>> 3) & 0xE0;
};

NV20GeForce.prototype.dispi_write_shadow = function(index, value, source)
{
    index &= 0xFFFF;
    value &= 0xFFFF;

    switch(index)
    {
        case 0:
            if(value >= 0xB0C0 && value <= NV20_DISPI_VERSION)
            {
                this.dispi_version = value;
            }
            break;
        case 1:
            this.dispi_width = Math.min(value, NV20_DISPI_MAX_XRES);
            this.dispi_apply_render_mode(source || "dispi-width");
            break;
        case 2:
            this.dispi_height = Math.min(value, NV20_DISPI_MAX_YRES);
            this.dispi_apply_render_mode(source || "dispi-height");
            break;
        case 3:
            this.dispi_bpp = value;
            this.dispi_apply_render_mode(source || "dispi-bpp");
            break;
        case 4:
            if(value & 1)
            {
                this.dispi_pending_disable = false;
                this.dispi_enable_value = value;
                this.dispi_apply_render_mode(source || "dispi-enable");
            }
            else if(this.render_active)
            {
                this.dispi_pending_disable = true;
            }
            else
            {
                this.dispi_pending_disable = false;
                this.dispi_enable_value = value;
            }
            break;
        case 5:
            this.dispi_bank = value;
            break;
        case 6:
            this.dispi_virtual_width = Math.min(value, NV20_DISPI_MAX_XRES);
            this.dispi_apply_render_mode(source || "dispi-virtual-width");
            break;
        case 7:
            this.dispi_virtual_height = Math.min(value, NV20_DISPI_MAX_YRES);
            break;
        case 8:
            this.dispi_offset_x = Math.min(value, NV20_DISPI_MAX_XRES);
            break;
        case 9:
            this.dispi_offset_y = Math.min(value, NV20_DISPI_MAX_YRES);
            break;
    }
};

NV20GeForce.prototype.dispi_turn_off_vga_svga = function()
{
    const vga = this.dispi_vga_delegate();

    if(!vga || !vga.svga_enabled)
    {
        return;
    }

    vga.svga_enabled = false;
    vga.dispi_enable_value = vga.dispi_enable_value & ~1;
    vga.svga_bank_offset = 0;

    if(vga.update_layers)
    {
        vga.update_layers();
    }
};

NV20GeForce.prototype.dispi_apply_render_mode = function(source)
{
    if(this.dispi_pending_disable)
    {
        return false;
    }

    const mode = this.dispi_render_mode();

    if(!mode)
    {
        return false;
    }

    if(!this.set_render_mode(mode.width, mode.height, mode.bpp, mode.stride, mode.offset, source || "dispi"))
    {
        return false;
    }

    this.dispi_mirror_crtc_mode(mode);
    this.dispi_turn_off_vga_svga();
    this.activate_rendering();
    this.render_mark_dirty_rect(0, 0, mode.width, mode.height);
    this.schedule_render();
    return true;
};

NV20GeForce.prototype.dispi_index_read8 = function()
{
    return this.dispi_index_read16() & 0xFF;
};

NV20GeForce.prototype.dispi_index_read16 = function()
{
    const vga = this.dispi_vga_delegate();

    if(!this.geforce_handles_dispi() && vga && vga.port1CE_read)
    {
        return vga.port1CE_read() & 0xFFFF;
    }

    return this.dispi_index & 0xFFFF;
};

NV20GeForce.prototype.dispi_index_write8 = function(value)
{
    this.dispi_index_write16(value);
};

NV20GeForce.prototype.dispi_index_write16 = function(value)
{
    const vga = this.dispi_vga_delegate();

    value &= 0xFFFF;
    this.dispi_index = value;

    if(!this.geforce_handles_dispi() && vga && vga.port1CE_write)
    {
        vga.port1CE_write(value);
        return;
    }
};

NV20GeForce.prototype.dispi_data_read_local = function()
{
    switch(this.dispi_index & 0xFFFF)
    {
        case 0:
            return this.dispi_version;
        case 1:
            return this.dispi_enable_value & 2 ? NV20_DISPI_MAX_XRES : this.dispi_width;
        case 2:
            return this.dispi_enable_value & 2 ? NV20_DISPI_MAX_YRES : this.dispi_height;
        case 3:
            return this.dispi_enable_value & 2 ? NV20_DISPI_MAX_BPP : this.dispi_bpp;
        case 4:
            return this.dispi_enable_value;
        case 5:
            return this.dispi_bank;
        case 6:
            return this.dispi_virtual_width || this.dispi_width || 1;
        case 7:
            return this.dispi_virtual_height || this.dispi_height || 1;
        case 8:
            return this.dispi_offset_x;
        case 9:
            return this.dispi_offset_y;
        case 0x0A:
            return this.vram_size / 0x10000 | 0;
    }

    return 0xFFFF;
};

NV20GeForce.prototype.dispi_data_read8 = function()
{
    return this.dispi_data_read16() & 0xFF;
};

NV20GeForce.prototype.dispi_data_read16 = function()
{
    const vga = this.dispi_vga_delegate();

    if(!this.geforce_handles_dispi() && vga && vga.port1CF_read)
    {
        return vga.port1CF_read() & 0xFFFF;
    }

    return this.dispi_data_read_local() & 0xFFFF;
};

NV20GeForce.prototype.dispi_data_write8 = function(value)
{
    const old_value = this.dispi_data_read_local();
    this.dispi_data_write16(old_value & 0xFF00 | value & 0xFF);
};

NV20GeForce.prototype.dispi_data_write16 = function(value)
{
    const vga = this.dispi_vga_delegate();
    const index = this.dispi_index & 0xFFFF;

    value &= 0xFFFF;

    if(!this.geforce_handles_dispi() && vga && vga.port1CF_write)
    {
        if(this.dispi_ignore_boot_mode_write(index, value))
        {
            this.dispi_report_geforce_mode();
            return;
        }

        this.dispi_note_delegated_vbe_write(index, value);
        vga.port1CF_write(value);
        this.dispi_write_shadow(index, value, "dispi-delegated");

        if(this.render_active)
        {
            this.dispi_turn_off_vga_svga();
        }

        this.dispi_report_geforce_mode();
        return;
    }

    if(!this.dispi_takeover_logged)
    {
        dbg_log(this.name + " owns Bochs DISPI/VBE ports", LOG_PCI);
        this.dispi_takeover_logged = true;
    }

    this.dispi_write_shadow(index, value, "dispi");
};

NV20GeForce.prototype.legacy_vga_crtc_data_is_local = function()
{
    return this.prmcio_crtc_index > NV20_VGA_CRTC_MAX ||
        this.geforce_owns_legacy_vga() && !this.dispi_delegated_crtc_active();
};

NV20GeForce.prototype.legacy_vga_crtc_data_is_dispi_delegated = function()
{
    return this.dispi_delegated_crtc_active();
};

NV20GeForce.prototype.legacy_vga_port_read_shadow = function(port)
{
    port &= 0xFFFF;

    switch(port)
    {
        case 0x3C0:
            return (this.legacy_vga_attr_index | this.legacy_vga_attr_palette_source) & 0xFF;
        case 0x3C1:
            return this.legacy_vga_attr_regs[this.legacy_vga_attr_index & 0x1F];
        case 0x3C2:
        case 0x3CA:
            return 0;
        case 0x3C3:
            return this.legacy_vga_enable;
        case 0x3C4:
            return this.legacy_vga_seq_index;
        case 0x3C5:
            return this.legacy_vga_seq_regs[this.legacy_vga_seq_index];
        case 0x3C6:
            return this.legacy_vga_dac_mask;
        case 0x3C7:
            return 0;
        case 0x3C8:
            return this.legacy_vga_dac_write / 3 & 0xFF;
        case 0x3C9:
            return this.legacy_vga_dac_data[this.legacy_vga_dac_read++ % this.legacy_vga_dac_data.length];
        case 0x3CC:
            return this.legacy_vga_misc_output;
        case 0x3CE:
            return this.legacy_vga_graphics_index;
        case 0x3CF:
            return this.legacy_vga_graphics_regs[this.legacy_vga_graphics_index];
        case 0x3BA:
        case 0x3DA:
            this.legacy_vga_attr_data_phase = false;
            return this.crtc_next_raster_status().status;
    }

    return -1;
};

NV20GeForce.prototype.legacy_vga_port_write_shadow = function(port, value)
{
    port &= 0xFFFF;
    value &= 0xFF;

    switch(port)
    {
        case 0x3C0:
            if(!this.legacy_vga_attr_data_phase)
            {
                this.legacy_vga_attr_index = value & 0x1F;
                this.legacy_vga_attr_palette_source = value & 0x20;
                this.legacy_vga_attr_data_phase = true;
            }
            else
            {
                this.legacy_vga_attr_regs[this.legacy_vga_attr_index & 0x1F] = value;
                this.legacy_vga_attr_data_phase = false;
            }
            return true;
        case 0x3C2:
            this.legacy_vga_misc_output = value;
            return true;
        case 0x3C3:
            this.legacy_vga_enable = value;
            return true;
        case 0x3C4:
            this.legacy_vga_seq_index = value;
            return true;
        case 0x3C5:
            this.legacy_vga_seq_regs[this.legacy_vga_seq_index] = value;
            return true;
        case 0x3C6:
            this.legacy_vga_dac_mask = value;
            return true;
        case 0x3C7:
            this.legacy_vga_dac_read = (value & 0xFF) * 3;
            return true;
        case 0x3C8:
            this.legacy_vga_dac_write = (value & 0xFF) * 3;
            return true;
        case 0x3C9:
            this.legacy_vga_dac_data[this.legacy_vga_dac_write++ % this.legacy_vga_dac_data.length] = value;
            return true;
        case 0x3CE:
            this.legacy_vga_graphics_index = value;
            return true;
        case 0x3CF:
            this.legacy_vga_graphics_regs[this.legacy_vga_graphics_index] = value;
            return true;
        case 0x3D8:
        case 0x3DA:
            return true;
    }

    return false;
};

NV20GeForce.prototype.legacy_vga_memory_read8 = function(addr)
{
    const vga = this.cpu && this.cpu.devices && this.cpu.devices.vga;

    if(!this.geforce_owns_legacy_vga() && vga)
    {
        return vga.vga_memory_read(addr);
    }

    return this.legacy_vga_memory[(addr - NV20_LEGACY_VGA_MEM_BASE) & (NV20_LEGACY_VGA_MEM_SIZE - 1)];
};

NV20GeForce.prototype.legacy_vga_memory_write8 = function(addr, value)
{
    const vga = this.cpu && this.cpu.devices && this.cpu.devices.vga;

    if(!this.geforce_owns_legacy_vga() && vga)
    {
        vga.vga_memory_write(addr, value);
        return;
    }

    this.legacy_vga_memory[(addr - NV20_LEGACY_VGA_MEM_BASE) & (NV20_LEGACY_VGA_MEM_SIZE - 1)] = value & 0xFF;
};

NV20GeForce.prototype.legacy_vga_port_read8 = function(port)
{
    return this.vga_port_read8(port);
};

NV20GeForce.prototype.legacy_vga_port_read16 = function(port)
{
    return this.vga_port_read16(port);
};

NV20GeForce.prototype.legacy_vga_read = function(port)
{
    port &= 0xFFFF;

    if(port === 0x3B4 || port === 0x3D4)
    {
        return this.prmcio_crtc_index;
    }

    if(port === 0x3B5 || port === 0x3D5)
    {
        if(this.legacy_vga_crtc_data_is_local())
        {
            return this.prmcio_read_crtc_data();
        }
    }

    return -1;
};

NV20GeForce.prototype.legacy_vga_write = function(port, value)
{
    port &= 0xFFFF;
    value &= 0xFF;

    if(port === 0x3B4 || port === 0x3D4)
    {
        return this.prmcio_set_crtc_index(value);
    }

    if(port === 0x3B5 || port === 0x3D5)
    {
        return this.prmcio_write_crtc_data(value);
    }

    return false;
};

NV20GeForce.prototype.vga_port_read8 = function(port)
{
    const vga = this.cpu && this.cpu.devices && this.cpu.devices.vga;

    port &= 0xFFFF;

    const crtc_value = this.legacy_vga_read(port);
    if(crtc_value >= 0)
    {
        return crtc_value;
    }

    if(this.geforce_owns_legacy_vga())
    {
        const shadow_value = this.legacy_vga_port_read_shadow(port);

        if(shadow_value >= 0)
        {
            return shadow_value;
        }
    }

    if(!vga)
    {
        const shadow_value = this.legacy_vga_port_read_shadow(port);
        return shadow_value >= 0 ? shadow_value : 0xFF;
    }

    switch(port)
    {
        case 0x3B4:
        case 0x3D4:
            return vga.port3D4_read();
        case 0x3B5:
        case 0x3D5:
            return vga.port3D5_read();
        case 0x3BA:
        case 0x3DA:
            return vga.port3DA_read();
        case 0x3C0:
            return vga.port3C0_read();
        case 0x3C1:
            return vga.port3C1_read();
        case 0x3C2:
            return 0x10;
        case 0x3C3:
            return 0;
        case 0x3C4:
            return vga.port3C4_read();
        case 0x3C5:
            return vga.port3C5_read();
        case 0x3C6:
            return vga.port3C6_read();
        case 0x3C7:
            return vga.port3C7_read();
        case 0x3C8:
            return vga.port3C8_read();
        case 0x3C9:
            return vga.port3C9_read();
        case 0x3CC:
            return vga.port3CC_read();
        case 0x3CE:
            return vga.port3CE_read();
        case 0x3CF:
            return vga.port3CF_read();
    }

    return -1;
};

NV20GeForce.prototype.vga_port_write8 = function(port, value)
{
    const vga = this.cpu && this.cpu.devices && this.cpu.devices.vga;

    port &= 0xFFFF;
    value &= 0xFF;

    switch(port)
    {
        case 0x3B4:
        case 0x3D4:
            this.prmcio_set_crtc_index(value);
            if(vga && value <= NV20_VGA_CRTC_MAX &&
                !this.dispi_suppressed_crtc_active() &&
                (!this.geforce_owns_legacy_vga() ||
                 this.dispi_delegated_crtc_writes > 0 && !this.render_active))
            {
                vga.port3D4_write(value);
            }
            return true;
        case 0x3B5:
        case 0x3D5:
            if(this.dispi_suppressed_crtc_active())
            {
                this.dispi_suppressed_crtc_writes--;

                if(this.dispi_suppressed_crtc_log_count < 4)
                {
                    this.dispi_suppressed_crtc_log_count++;
                    dbg_log(this.name + " suppressed Bochs VBE CRTC write index=" +
                            h(this.prmcio_crtc_index, 2), LOG_PCI);
                }

                this.dispi_report_geforce_mode();
            }
            else if(this.legacy_vga_crtc_data_is_dispi_delegated())
            {
                this.prmcio_crtc_regs[this.prmcio_crtc_index] = value;

                if(vga && !this.render_active)
                {
                    vga.port3D5_write(value);
                }

                this.dispi_delegated_crtc_writes--;

                if(this.dispi_delegated_crtc_log_count < 4)
                {
                    this.dispi_delegated_crtc_log_count++;
                    dbg_log(this.name + " delegated Bochs VBE CRTC write index=" +
                            h(this.prmcio_crtc_index, 2), LOG_PCI);
                }

                this.dispi_report_geforce_mode();
            }
            else if(this.legacy_vga_crtc_data_is_local() || !vga)
            {
                this.dispi_clear_delegated_crtc();
                this.prmcio_write_crtc_data(value);
            }
            else
            {
                this.prmcio_crtc_regs[this.prmcio_crtc_index] = value;
                vga.port3D5_write(value);
            }
            return true;
    }

    const shadow_handled = this.legacy_vga_port_write_shadow(port, value);

    if(this.geforce_owns_legacy_vga())
    {
        return shadow_handled;
    }

    if(!vga)
    {
        return shadow_handled || this.legacy_vga_write(port, value);
    }

    switch(port)
    {
        case 0x3C0:
            vga.port3C0_write(value);
            return true;
        case 0x3C2:
            vga.port3C2_write(value);
            return true;
        case 0x3C3:
            return true;
        case 0x3C4:
            vga.port3C4_write(value);
            return true;
        case 0x3C5:
            vga.port3C5_write(value);
            return true;
        case 0x3C6:
            vga.port3C6_write(value);
            return true;
        case 0x3C7:
            vga.port3C7_write(value);
            return true;
        case 0x3C8:
            vga.port3C8_write(value);
            return true;
        case 0x3C9:
            vga.port3C9_write(value);
            return true;
        case 0x3CE:
            vga.port3CE_write(value);
            return true;
        case 0x3CF:
            vga.port3CF_write(value);
            return true;
        case 0x3D8:
        case 0x3DA:
            return true;
    }

    return false;
};

NV20GeForce.prototype.vga_port_read16 = function(port)
{
    var low = this.vga_port_read8(port);
    var high = this.vga_port_read8(port + 1);

    if(low < 0)
    {
        low = 0xFF;
    }

    if(high < 0)
    {
        high = 0xFF;
    }

    return low | high << 8;
};

NV20GeForce.prototype.vga_port_write16 = function(port, value)
{
    value &= 0xFFFF;

    this.vga_port_write8(port, value & 0xFF);

    if(port === 0x3B4 || port === 0x3D4)
    {
        this.vga_port_write8(port + 1, value >> 8 & 0xFF);
    }

    return true;
};

NV20GeForce.prototype.legacy_vga_read8_3b4 = function() { return this.vga_port_read8(0x3B4); };
NV20GeForce.prototype.legacy_vga_read8_3b5 = function() { return this.vga_port_read8(0x3B5); };
NV20GeForce.prototype.legacy_vga_read8_3d4 = function() { return this.vga_port_read8(0x3D4); };
NV20GeForce.prototype.legacy_vga_read8_3d5 = function() { return this.vga_port_read8(0x3D5); };
NV20GeForce.prototype.legacy_vga_read16_3b4 = function() { return this.vga_port_read16(0x3B4); };
NV20GeForce.prototype.legacy_vga_read16_3b5 = function() { return this.vga_port_read16(0x3B5); };
NV20GeForce.prototype.legacy_vga_read16_3d4 = function() { return this.vga_port_read16(0x3D4); };
NV20GeForce.prototype.legacy_vga_read16_3d5 = function() { return this.vga_port_read16(0x3D5); };
NV20GeForce.prototype.legacy_vga_write8_3b4 = function(value) { return this.vga_port_write8(0x3B4, value); };
NV20GeForce.prototype.legacy_vga_write8_3b5 = function(value) { return this.vga_port_write8(0x3B5, value); };
NV20GeForce.prototype.legacy_vga_write8_3d4 = function(value) { return this.vga_port_write8(0x3D4, value); };
NV20GeForce.prototype.legacy_vga_write8_3d5 = function(value) { return this.vga_port_write8(0x3D5, value); };
NV20GeForce.prototype.legacy_vga_write16_3b4 = function(value) { return this.vga_port_write16(0x3B4, value); };
NV20GeForce.prototype.legacy_vga_write16_3b5 = function(value) { return this.vga_port_write16(0x3B5, value); };
NV20GeForce.prototype.legacy_vga_write16_3d4 = function(value) { return this.vga_port_write16(0x3D4, value); };
NV20GeForce.prototype.legacy_vga_write16_3d5 = function(value) { return this.vga_port_write16(0x3D5, value); };

NV20GeForce.prototype.mmio_vga_alias_read8 = function(offset)
{
    const port = nv20_mmio_vga_alias_port(offset);

    if(port < 0)
    {
        return -1;
    }

    if(port === 0x3BA || port === 0x3DA)
    {
        return this.legacy_vga_port_read_shadow(port);
    }

    if(nv20_mmio_vga_alias_head(offset))
    {
        return 0;
    }

    return this.vga_port_read8(port);
};

NV20GeForce.prototype.mmio_vga_alias_write8 = function(offset, value)
{
    const port = nv20_mmio_vga_alias_port(offset);

    if(port < 0)
    {
        return false;
    }

    if(nv20_mmio_vga_alias_head(offset))
    {
        return true;
    }

    return this.vga_port_write8(port, value);
};

NV20GeForce.prototype.prmcio_read8 = function(offset)
{
    offset = offset >>> 0;

    if(offset === NV20_PRMCIO_CRTC_INDEX_COLOR || offset === NV20_PRMCIO_CRTC_INDEX_MONO)
    {
        return this.legacy_vga_read(0x3D4);
    }

    if(offset === NV20_PRMCIO_CRTC_DATA_COLOR || offset === NV20_PRMCIO_CRTC_DATA_MONO)
    {
        return this.prmcio_read_crtc_data();
    }

    return -1;
};

NV20GeForce.prototype.prmcio_write8 = function(offset, value)
{
    offset = offset >>> 0;
    value &= 0xFF;

    if(offset === NV20_PRMCIO_CRTC_INDEX_COLOR || offset === NV20_PRMCIO_CRTC_INDEX_MONO)
    {
        return this.prmcio_set_crtc_index(value);
    }

    if(offset === NV20_PRMCIO_CRTC_DATA_COLOR || offset === NV20_PRMCIO_CRTC_DATA_MONO)
    {
        return this.prmcio_write_crtc_data(value);
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

NV20GeForce.prototype.mmio_note_hot_poll = function(offset, value, width)
{
    offset >>>= 0;
    value >>>= 0;
    width = width || 32;

    if(!DEBUG || !this.missing_trace || !nv20_mmio_hot_poll_read_offset(offset))
    {
        return;
    }

    const key = h(offset, 6) + ":" + width;
    const count = (this.mmio_hot_poll_counts.get(key) || 0) + 1;
    this.mmio_hot_poll_counts.set(key, count);

    const name = nv20_mmio_register_name(offset);

    this["debug_mmio_hot_poll"] = {
        "offset": offset,
        "name": name || null,
        "width": width,
        "value": value,
        "count": count,
        "master_intr": this.get_master_interrupt_status(),
        "fifo_cache_error": this.fifo_cache_error,
        "fifo_intr": this.fifo_intr,
        "fifo_intr_en": this.fifo_intr_en,
        "fifo_get": this.fifo_get,
        "fifo_put": this.fifo_cache1_put,
        "fifo_dma_get": this.fifo_dma_get,
        "fifo_dma_put": this.fifo_dma_put,
        "fifo_wait_notify": this.fifo_wait_notify,
        "fifo_wait_flip": this.fifo_wait_flip,
        "fifo_wait_soft": this.fifo_wait_soft,
        "fifo_wait_acquire": this.fifo_wait_acquire,
        "graph_intr": this.graph_intr,
        "graph_intr_en": this.graph_intr_en,
        "graph_status": this.graph_status,
        "crtc_intr": this.crtc_intr,
        "crtc_intr_en": this.crtc_intr_en,
        "crtc_index": this.prmcio_crtc_index,
    };

    if(!nv20_mmio_hot_poll_log_count(count))
    {
        return;
    }

    if(this.mmio_hot_poll_log_count >= this.mmio_hot_poll_log_limit)
    {
        if(!this.mmio_hot_poll_suppressed)
        {
            this.mmio_hot_poll_suppressed = true;
            dbg_log(this.name + " mmio hot poll log suppressed" +
                    " unique=" + this.mmio_hot_poll_counts.size,
                    LOG_PCI);
        }

        return;
    }

    this.mmio_hot_poll_log_count++;

    dbg_log(this.name + " mmio hot poll read" + width + " " + h(offset, 6) +
            (name ? " (" + name + ")" : "") +
            " count=" + count +
            " value=" + h(value, width <= 8 ? 2 : 8) +
            " master=" + h(this.get_master_interrupt_status(), 8) +
            " fifo_cache_error=" + h(this.fifo_cache_error, 8) +
            " fifo_intr=" + h(this.fifo_intr, 8) +
            "/" + h(this.fifo_intr_en, 8) +
            " fifo_get_put=" + h(this.fifo_get, 8) +
            "/" + h(this.fifo_cache1_put, 8) +
            " dma_get_put=" + h(this.fifo_dma_get, 8) +
            "/" + h(this.fifo_dma_put, 8) +
            " fifo_wait=" +
            (this.fifo_wait_notify ? "n" : "-") +
            (this.fifo_wait_flip ? "f" : "-") +
            (this.fifo_wait_soft ? "s" : "-") +
            (this.fifo_wait_acquire ? "a" : "-") +
            " graph_intr=" + h(this.graph_intr, 8) +
            "/" + h(this.graph_intr_en, 8) +
            " graph_status=" + h(this.graph_status, 8) +
            " crtc_intr=" + h(this.crtc_intr, 8) +
            "/" + h(this.crtc_intr_en, 8) +
            " crtc_index=" + h(this.prmcio_crtc_index, 2),
            LOG_PCI);
};

NV20GeForce.prototype.mmio_read8 = function(offset)
{
    offset = offset & (NV20_MMIO_SIZE - 1);

    const alias_value = this.mmio_vga_alias_read8(offset);
    if(alias_value >= 0)
    {
        this.mmio_note_hot_poll(offset, alias_value, 8);
        return alias_value;
    }

    const value = this.prmcio_read8(offset);

    if(value >= 0)
    {
        this.mmio_note_hot_poll(offset, value, 8);
        return value;
    }

    return this.mmio_read32(offset & ~3) >>> ((offset & 3) << 3) & 0xFF;
};

NV20GeForce.prototype.mmio_write8 = function(offset, value)
{
    offset = offset & (NV20_MMIO_SIZE - 1);

    if(this.mmio_vga_alias_write8(offset, value))
    {
        this.mmio_log("write", offset & ~3, this.register_read32(offset & ~3).value, true);
        return;
    }

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

    if(nv20_mmio_vga_alias_port(offset) >= 0)
    {
        const value =
            this.mmio_vga_alias_read8(offset) |
            this.mmio_vga_alias_read8(offset + 1) << 8 |
            this.mmio_vga_alias_read8(offset + 2) << 16 |
            this.mmio_vga_alias_read8(offset + 3) << 24;
        this.mmio_log("read", offset, value >>> 0, true);
        this.mmio_note_hot_poll(offset, value >>> 0, 32);
        return value >>> 0;
    }

    const prmcio_value = this.prmcio_read32(offset);

    if(prmcio_value >= 0)
    {
        this.mmio_log("read", offset, prmcio_value, true);
        this.mmio_note_hot_poll(offset, prmcio_value, 32);
        return prmcio_value;
    }

    const result = this.register_read32(offset);
    this.mmio_log("read", offset, result.value, result.known);
    this.mmio_note_hot_poll(offset, result.value, 32);
    return result.value;
};

NV20GeForce.prototype.mmio_write32 = function(offset, value)
{
    offset = offset & (NV20_MMIO_SIZE - 1) & ~3;
    value = value >>> 0;

    if(nv20_mmio_vga_alias_port(offset) >= 0)
    {
        this.mmio_vga_alias_write8(offset, value);
        this.mmio_vga_alias_write8(offset + 1, value >>> 8);
        this.mmio_vga_alias_write8(offset + 2, value >>> 16);
        this.mmio_vga_alias_write8(offset + 3, value >>> 24);
        this.mmio_log("write", offset, value, true);
        return;
    }

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
    else if(offset >= NV20_MMIO_ROM_BASE && offset < NV20_MMIO_ROM_BASE + NV20_MMIO_ROM_SIZE)
    {
        value = this.pci_config_space8 && this.pci_config_space8[0x50] === 0 ?
            this.pci_rom_read32(offset - NV20_MMIO_ROM_BASE) : 0;
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
                value = this.fifo_dma_push_read();
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

            case 0x008100:
                value = this.video_intr;
                break;
            case 0x008140:
            case 0x200140:
                value = this.video_intr_en;
                break;
            case 0x008704:
                value = this.video_stop;
                break;

            case 0x100000:
                value = this.fb_boot_0;
                break;
            case 0x100200:
                value = this.fb_cfg;
                break;
            case 0x10020C:
                value = this.vram_size;
                break;
            case 0x100240:
            case 0x400900:
                value = this.fb_tile0_flags;
                break;
            case 0x100244:
            case 0x400904:
                value = this.fb_tile0_limit;
                break;
            case 0x100248:
            case 0x400908:
                value = this.fb_tile0_pitch;
                break;
            case 0x100320:
                value = this.fb_cfg0;
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
            case 0x400144:
                // We do not emulate real PGRAPH context switching yet. Hide the
                // context-control bits so legacy Windows drivers do not wait
                // on status transitions that can never complete.
                value = 0;
                break;
            case 0x400148:
                value = this.graph_ctx_user;
                break;
            case 0x40014C:
                value = this.graph_ctx_switch1;
                break;
            case 0x400150:
                value = this.graph_ctx_switch2;
                break;
            case 0x400154:
                value = this.graph_ctx_switch3;
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
            case 0x40070C:
                value = this.graph_trapped_data_high;
                break;
            case 0x400718:
                value = this.graph_notify;
                break;
            case 0x40071C:
                value = this.graph_notify_instance;
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
            {
                const raster = this.crtc_next_raster_status();
                // Bochs exposes VGA status through this register. In
                // particular, VGA status bit 3 becomes bit 16 here; XP's
                // driver polls it while waiting for retrace.
                value = (raster.status << 13) | raster.line;
                break;
            }
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
                value = this.crtc_engine_ctrl;
                break;
            case 0x6013B4:
            case 0x6013D4:
                value = this.prmcio_read32(offset);
                break;

            case 0x680300:
                value = this.ramdac_cursor_start;
                break;
            case 0x680404:
                value = this.ramdac_fp_tg_control;
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
                value = this.ramdac_dacclk;
                break;

            default:
                if((offset >= NV20_PVIDEO_BASE && offset < NV20_PVIDEO_BASE + NV20_PVIDEO_SIZE) ||
                   (offset >= NV20_PVIDEO_OVERLAY_BASE && offset < NV20_PVIDEO_OVERLAY_BASE + NV20_PVIDEO_OVERLAY_SIZE))
                {
                    value = this.pvideo_regs.get(offset) || 0;
                }
                else
                {
                    known = !!nv20_mmio_register_name(offset);
                    value = this.mmio_registers.get(offset) || 0;
                }
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
        const ramin_offset = offset - NV20_PRAMIN_BASE;
        this.ramin_write32(ramin_offset, value);
        this.fifo_note_pramin_write(ramin_offset);
        return true;
    }

    if(offset >= NV20_MMIO_ROM_BASE && offset < NV20_MMIO_ROM_BASE + NV20_MMIO_ROM_SIZE)
    {
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
            this.update_irq_level();
            return true;
        case 0x000140:
            this.mc_intr_en = value;
            this.update_irq_level();
            return true;
        case 0x000200:
            this.mc_enable = value;
            return true;

        case 0x001100:
            this.bus_intr &= ~value;
            this.update_irq_level();
            return true;
        case 0x001140:
            this.bus_intr_en = value;
            this.update_irq_level();
            return true;

        case 0x002080:
            this.fifo_cache_error &= ~value;
            return true;
        case 0x002100:
            this.fifo_intr &= ~value;
            this.update_irq_level();
            return true;
        case 0x002140:
            this.fifo_intr_en = value;
            this.update_irq_level();
            return true;
        case 0x002200:
            this.fifo_config = value;
            return true;
        case 0x002210:
            this.fifo_ramht = value;
            return true;
        case 0x002214:
            this.fifo_ramfc = value;
            this.fifo_reset_all_channel_state(false);
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
        {
            const old_mode = this.fifo_mode;
            this.fifo_mode = value;
            if(value === 0)
            {
                this.fifo_reset_all_channel_state(true);
            }
            if(((old_mode | value) >>> 0) !== (old_mode >>> 0))
            {
                this.fifo_kick_enabled_dma_channels("mode");
            }
            return true;
        }
        case 0x003200:
            this.fifo_cache1_push0 = value;
            this.fifo_kick_enabled_dma_channels("push0");
            return true;
        case 0x003204:
            this.fifo_cache1_push1 = value;
            this.fifo_active_channel = value & 0x1F;
            return true;
        case 0x003210:
            this.fifo_cache1_put = value & NV20_FIFO_CACHE_GET_MASK;
            return true;
        case 0x003218:
            this.fifo_dma_dcount = value;
            channel = this.fifo_active_channel_state();
            channel.dma_dcount = value;
            channel.context_loaded = true;
            return true;
        case 0x003220:
            this.fifo_dma_push = value & NV20_FIFO_DMA_PUSH_CONTROL_MASK;
            if((this.fifo_dma_push & NV20_FIFO_DMA_PUSH_ACCESS) &&
                !(this.fifo_dma_push & NV20_FIFO_DMA_PUSH_STATUS_SUSPENDED))
            {
                this.fifo_kick_enabled_dma_channels("dma-push");
            }
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
            channel.dma_state = value;
            channel.context_loaded = true;
            return true;
        case 0x003240:
            this.fifo_dma_put = value;
            channel = this.fifo_active_channel_state();
            channel.dma_put = value;
            channel.context_loaded = true;
            if(this.fifo_dma_push & NV20_FIFO_DMA_PUSH_ACCESS)
            {
                this.fifo_dma_kick(channel, "dma-put", NV20_FIFO_DMA_SYNC_KICK_LIMIT);
            }
            return true;
        case 0x003244:
            this.fifo_dma_get = value;
            channel = this.fifo_active_channel_state();
            channel.dma_get = value;
            channel.context_loaded = true;
            this.fifo_update_dma_push_state();
            return true;
        case 0x003248:
            this.fifo_ref_cnt = value;
            channel = this.fifo_active_channel_state();
            channel.ref = value;
            channel.context_loaded = true;
            return true;
        case 0x003250:
            this.fifo_pull0 = value;
            this.fifo_kick_enabled_dma_channels("pull0");
            return true;
        case 0x003254:
            this.fifo_pull1 = value;
            return true;
        case 0x003270:
            this.fifo_get = value & NV20_FIFO_CACHE_GET_MASK;
            this.fifo_intr &= ~NV20_FIFO_INTR_CACHE_ERROR;
            if(this.fifo_get === this.fifo_cache1_put)
            {
                this.fifo_pull0 &= ~0x100;
            }
            this.update_irq_level();
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
            this.update_irq_level();
            return true;
        case 0x009140:
            this.timer_intr_en = value;
            this.update_irq_level();
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

        case 0x008100:
            this.video_intr &= ~value;
            return true;
        case 0x008140:
        case 0x200140:
            this.video_intr_en = value;
            return true;
        case 0x008704:
            this.video_stop = value;
            return true;

        case 0x100000:
            // Strap-derived memory configuration register. It is effectively
            // read-only for the guest, but acknowledge writes like hardware.
            return true;
        case 0x100200:
            this.fb_cfg = value;
            return true;
        case 0x100320:
            this.fb_cfg0 = value;
            return true;
        case 0x100240:
        case 0x400900:
            this.fb_tile0_flags = value;
            return true;
        case 0x100244:
        case 0x400904:
            this.fb_tile0_limit = value;
            return true;
        case 0x100248:
        case 0x400908:
            this.fb_tile0_pitch = value & 0xFFFF;
            return true;

        case 0x101000:
            this.straps0_primary = value & 0x80000000 ? value : this.straps0_primary_original;
            return true;

        case 0x400100:
            this.graph_intr &= ~value;
            this.update_irq_level();

            if(this.fifo_wait_notify && this.graph_intr === 0)
            {
                this.fifo_wait_notify = false;
                this.fifo_resume_wait("notify");
            }

            return true;
        case 0x400108:
            this.graph_nsource = value;
            return true;
        case 0x400140:
            this.graph_intr_en = value;
            this.update_irq_level();
            return true;
        case 0x400144:
            this.graph_ctx_control = value;
            return true;
        case 0x400148:
            this.graph_ctx_user = value;
            return true;
        case 0x40014C:
            this.graph_ctx_switch1 = value;
            return true;
        case 0x400150:
            this.graph_ctx_switch2 = value;
            return true;
        case 0x400154:
            this.graph_ctx_switch3 = value;
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
        case 0x40070C:
            this.graph_trapped_data_high = value;
            return true;
        case 0x400718:
            this.graph_notify = value;
            return true;
        case 0x40071C:
            if(value & 0x00000002)
            {
                this.graph_flip_modulo = this.graph_flip_modulo || 2;
                this.graph_flip_read = (this.graph_flip_read + 1) % this.graph_flip_modulo;

                if(this.fifo_wait_flip && this.graph_flip_read !== this.graph_flip_write)
                {
                    this.fifo_wait_flip = false;
                    this.fifo_resume_wait("flip");
                }
            }

            this.graph_notify_instance = value;
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
            this.update_irq_level();
            return true;
        case 0x600140:
            this.crtc_intr_en = value;
            this.update_irq_level();
            return true;
        case 0x600800:
            this.dispi_clear_delegated_crtc();
            this.crtc_start = value;
            this.update_render_mode_from_crtc("pcrtc_start");
            return true;
        case 0x600804:
            this.dispi_clear_delegated_crtc();
            this.crtc_config = value;
            this.update_render_mode_from_crtc("pcrtc_config");
            return true;
        case 0x60080C:
            this.crtc_cursor_offset = value;
            this.hw_cursor_update(true);
            return true;
        case 0x600810:
            this.crtc_cursor_config = value;
            this.hw_cursor_update(true);
            return true;
        case 0x60081C:
            this.crtc_gpio_ext = value;
            return true;
        case 0x600868:
            this.crtc_engine_ctrl = value;
            return true;
        case 0x6013B4:
        case 0x6013D4:
            this.prmcio_write32(offset, value);
            return true;

        case 0x680300:
            this.ramdac_cursor_start = value;
            this.hw_cursor_update(true);
            return true;
        case 0x680404:
            this.ramdac_fp_tg_control = value;
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
        case 0x680828:
            this.ramdac_dacclk = value;
            return true;
    }

    if((offset >= NV20_PVIDEO_BASE && offset < NV20_PVIDEO_BASE + NV20_PVIDEO_SIZE) ||
       (offset >= NV20_PVIDEO_OVERLAY_BASE && offset < NV20_PVIDEO_OVERLAY_BASE + NV20_PVIDEO_OVERLAY_SIZE))
    {
        if(value)
        {
            this.pvideo_regs.set(offset, value);
        }
        else
        {
            this.pvideo_regs.delete(offset);
        }

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
    if(!known)
    {
        this.log_missing_command("mmio-" + kind,
            h(offset >>> 0, 6),
            "offset=" + h(offset >>> 0, 6) +
            (kind === "read" ? " value=" : " data=") + h(value >>> 0, 8));
    }

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

NV20GeForce.prototype.update_irq_level = function()
{
    const level = !!(((this.mc_intr_en & 1) && this.get_master_interrupt_status()) ||
                     ((this.mc_intr_en & 2) && this.mc_soft_intr));

    if(level === this.irq_level)
    {
        return;
    }

    this.irq_level = level;

    if(!this.pci)
    {
        return;
    }

    if(level)
    {
        this.pci.raise_irq(this.pci_id);
    }
    else
    {
        this.pci.lower_irq(this.pci_id);
    }
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

NV20GeForce.prototype.restore_pci_readonly_config = function()
{
    const space = this.pci_config_space8;

    if(!space)
    {
        return;
    }

    space[0x00] = NV20_VENDOR_ID & 0xFF;
    space[0x01] = NV20_VENDOR_ID >> 8;
    space[0x02] = NV20_DEVICE_ID_GEFORCE3_TI_500 & 0xFF;
    space[0x03] = NV20_DEVICE_ID_GEFORCE3_TI_500 >> 8;
    space[0x06] = 0xB0;
    space[0x07] = 0x02;
    space[0x08] = 0xA3;
    space[0x09] = 0x00;
    space[0x0A] = 0x00;
    space[0x0B] = 0x03;
    space[0x0E] = 0x00;

    space[0x2C] = NV20_SUBSYSTEM_VENDOR_ID & 0xFF;
    space[0x2D] = NV20_SUBSYSTEM_VENDOR_ID >> 8;
    space[0x2E] = NV20_SUBSYSTEM_ID_GEFORCE3_TI_500 & 0xFF;
    space[0x2F] = NV20_SUBSYSTEM_ID_GEFORCE3_TI_500 >> 8;
    space[0x34] = 0x60;
    space[0x35] = 0x00;
    space[0x36] = 0x00;
    space[0x37] = 0x00;
    space[0x3D] = 0x01;

    space[0x40] = space[0x2C];
    space[0x41] = space[0x2D];
    space[0x42] = space[0x2E];
    space[0x43] = space[0x2F];

    space[0x44] = 0x02;
    space[0x45] = 0x00;
    space[0x46] = 0x20;
    space[0x47] = 0x00;
    space[0x48] = 0x07;
    space[0x49] = 0x00;
    space[0x4A] = 0x00;
    space[0x4B] = 0x1F;

    space[0x54] = 0x01;
    space[0x55] = 0x00;
    space[0x56] = 0x00;
    space[0x57] = 0x00;

    space[0x60] = 0x01;
    space[0x61] = 0x44;
    space[0x62] = 0x02;
    space[0x63] = 0x00;
};

NV20GeForce.prototype.pci_on_config_write = function()
{
    this.restore_pci_readonly_config();
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
