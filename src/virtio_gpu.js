// https://docs.oasis-open.org/virtio/virtio/v1.3/csd01/virtio-v1.3-csd01.html#x1-3960007

import { VirtIO, VIRTIO_F_VERSION_1 } from "./virtio.js";
import { LOG_VIRTIO } from "./const.js";
import { dbg_log, LOG_LEVEL } from "./log.js";

// For Types Only
import { CPU } from "./cpu.js";
import { BusConnector } from "./bus.js";

// virtio-gpu command types
const VIRTIO_GPU_CMD_GET_DISPLAY_INFO        = 0x0100;
const VIRTIO_GPU_CMD_RESOURCE_CREATE_2D      = 0x0101;
const VIRTIO_GPU_CMD_RESOURCE_UNREF          = 0x0102;
const VIRTIO_GPU_CMD_SET_SCANOUT             = 0x0103;
const VIRTIO_GPU_CMD_RESOURCE_FLUSH          = 0x0104;
const VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D     = 0x0105;
const VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING = 0x0106;
const VIRTIO_GPU_CMD_RESOURCE_DETACH_BACKING = 0x0107;
const VIRTIO_GPU_CMD_UPDATE_CURSOR           = 0x0300;
const VIRTIO_GPU_CMD_MOVE_CURSOR             = 0x0301;

// response types
const VIRTIO_GPU_RESP_OK_NODATA       = 0x1100;
const VIRTIO_GPU_RESP_OK_DISPLAY_INFO = 0x1101;
const VIRTIO_GPU_RESP_ERR_UNSPEC      = 0x1200;

const VIRTIO_GPU_MAX_SCANOUTS = 16;
const VIRTIO_GPU_EVENT_DISPLAY = 1 << 0;

// virtio_gpu_formats
const VIRTIO_GPU_FORMAT_B8G8R8A8_UNORM = 1;
const VIRTIO_GPU_FORMAT_B8G8R8X8_UNORM = 2;

const VIRTIO_GPU_FLAG_FENCE = 0x01;

const VIRTIO_GPU_CMD_NAMES = new Map([
    [VIRTIO_GPU_CMD_GET_DISPLAY_INFO, "GET_DISPLAY_INFO"],
    [VIRTIO_GPU_CMD_RESOURCE_CREATE_2D, "RESOURCE_CREATE_2D"],
    [VIRTIO_GPU_CMD_RESOURCE_UNREF, "RESOURCE_UNREF"],
    [VIRTIO_GPU_CMD_SET_SCANOUT, "SET_SCANOUT"],
    [VIRTIO_GPU_CMD_RESOURCE_FLUSH, "RESOURCE_FLUSH"],
    [VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D, "TRANSFER_TO_HOST_2D"],
    [VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING, "RESOURCE_ATTACH_BACKING"],
    [VIRTIO_GPU_CMD_RESOURCE_DETACH_BACKING, "RESOURCE_DETACH_BACKING"],
    [VIRTIO_GPU_CMD_UPDATE_CURSOR, "UPDATE_CURSOR"],
    [VIRTIO_GPU_CMD_MOVE_CURSOR, "MOVE_CURSOR"],
]);

/**
 * @constructor
 * @param {CPU} cpu
 * @param {BusConnector} bus
 * @param {*} vga
 */
export function VirtioGPU(cpu, bus, vga)
{
    this.bus = bus;
    this.vga = vga || null;
    this.events_read = 0;
    this.resources = new Map();
    this.scanouts = new Map();

    this.claimed_width = 0;
    this.claimed_height = 0;
    this.display_image_data = null;
    this.display_present_image_data = null;
    this.display_width = 0;
    this.display_height = 0;
    this.display_has_content = false;
    this.vga_provider_registered = false;
    this.vga_provider = () => this.get_vga_provider_frame();
    this.debug_last_frame_log_ms = 0;
    this.debug_last_frame_hash = 0;

    this.cursor = {
        scanout_id: 0,
        x: 0,
        y: 0,
        resource_id: 0,
        hot_x: 0,
        hot_y: 0,
        visible: false,
    };

    /** @type {VirtIO} */
    this.virtio = new VirtIO(cpu, {
        name: "virtio-gpu",
        pci_id: 0x0D << 3,
        device_id: 0x1050,
        subsystem_device_id: 16,

        common: {
            initial_port: 0xE800,
            queues: [
                { size_supported: 256, notify_offset: 0 },
                { size_supported: 16,  notify_offset: 1 },
            ],
            features: [
                VIRTIO_F_VERSION_1,
            ],
            on_driver_ok: () => {
                this.raise_display_event();
            },
        },

        notification: {
            initial_port: 0xE900,
            single_handler: false,
            handlers: [
                () => {
                    this.handle_ctrlq();
                },
                () => {
                    this.handle_cursorq();
                },
            ],
        },

        isr_status: { initial_port: 0xE700 },

        device_specific: {
            initial_port: 0xE600,
            struct: createGPUConfigStruct(this),
        },
    });
}

VirtioGPU.prototype.raise_display_event = function()
{
    this.events_read |= VIRTIO_GPU_EVENT_DISPLAY;
    this.virtio.notify_config_changes();
};

VirtioGPU.prototype.handle_ctrlq = function()
{
    const q = this.virtio.queues[0];

    while(q.has_request())
    {
        const chain = q.pop_request();
        const req_buf = new Uint8Array(chain.length_readable);
        chain.get_next_blob(req_buf);

        const view = new DataView(req_buf.buffer);
        const hdr = parse_ctrl_hdr(view, 0);
        this.debug_log_ctrl_command(view, req_buf.length, hdr);

        let resp = null;
        switch(hdr.type)
        {
            case VIRTIO_GPU_CMD_GET_DISPLAY_INFO:
                resp = this.cmd_get_display_info(hdr);
                break;
            case VIRTIO_GPU_CMD_RESOURCE_CREATE_2D:
                resp = this.cmd_resource_create_2d(view, hdr);
                break;
            case VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING:
                resp = this.cmd_attach_backing(view, hdr);
                break;
            case VIRTIO_GPU_CMD_RESOURCE_DETACH_BACKING:
                resp = this.cmd_detach_backing(view, hdr);
                break;
            case VIRTIO_GPU_CMD_RESOURCE_UNREF:
                resp = this.cmd_resource_unref(view, hdr);
                break;
            case VIRTIO_GPU_CMD_SET_SCANOUT:
                resp = this.cmd_set_scanout(view, hdr);
                break;
            case VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D:
                resp = this.cmd_transfer_to_host_2d(view, hdr);
                break;
            case VIRTIO_GPU_CMD_RESOURCE_FLUSH:
                resp = this.cmd_resource_flush(view, hdr);
                break;
            default:
                console.warn("virtio-gpu: unknown control command", hdr.type);
                resp = this.resp_err(hdr);
        }

        chain.set_next_blob(resp);
        q.push_reply(chain);
    }

    q.flush_replies();
};

VirtioGPU.prototype.handle_cursorq = function()
{
    const q = this.virtio.queues[1];

    while(q.has_request())
    {
        const chain = q.pop_request();
        const req_buf = new Uint8Array(chain.length_readable);
        chain.get_next_blob(req_buf);

        const view = new DataView(req_buf.buffer);
        const hdr = parse_ctrl_hdr(view, 0);
        this.debug_log_ctrl_command(view, req_buf.length, hdr);
        let resp = this.resp_ok_nodata(hdr);

        switch(hdr.type)
        {
            case VIRTIO_GPU_CMD_UPDATE_CURSOR:
                if(req_buf.length >= 56)
                {
                    this.cursor.scanout_id = view.getUint32(24, true);
                    this.cursor.x = view.getUint32(28, true);
                    this.cursor.y = view.getUint32(32, true);
                    this.cursor.resource_id = view.getUint32(40, true);
                    this.cursor.hot_x = view.getUint32(44, true);
                    this.cursor.hot_y = view.getUint32(48, true);
                    this.cursor.visible = this.cursor.resource_id !== 0;
                }
                this.present_display_surface();
                break;

            case VIRTIO_GPU_CMD_MOVE_CURSOR:
                if(req_buf.length >= 40)
                {
                    this.cursor.scanout_id = view.getUint32(24, true);
                    this.cursor.x = view.getUint32(28, true);
                    this.cursor.y = view.getUint32(32, true);
                }
                this.present_display_surface();
                break;

            default:
                console.warn("virtio-gpu: unknown cursor command", hdr.type);
                resp = this.resp_err(hdr);
        }

        chain.set_next_blob(resp);
        q.push_reply(chain);
    }

    q.flush_replies();
};

VirtioGPU.prototype.debug_log_ctrl_command = function(view, length, hdr)
{
    const name = VIRTIO_GPU_CMD_NAMES.get(hdr.type) || "UNKNOWN";
    const details = debug_gpu_command_details(view, length, hdr.type);
    const message = "virtio-gpu: " + name +
        " type=0x" + hdr.type.toString(16) +
        " len=" + length +
        " flags=0x" + hdr.flags.toString(16) +
        " fence=" + hdr.fence_id.toString() +
        " ctx=" + hdr.ctx_id +
        (details ? " " + details : "");

    if(LOG_LEVEL & LOG_VIRTIO)
    {
        dbg_log(message, LOG_VIRTIO);
    }
    else
    {
        console.log(message);
    }
};

VirtioGPU.prototype.debug_log_frame_stats = function(resource, scanout)
{
    const stats = debug_gpu_frame_stats(resource, scanout);
    const now = Date.now();

    if(stats.hash === this.debug_last_frame_hash && now - this.debug_last_frame_log_ms < 1000)
    {
        return;
    }

    this.debug_last_frame_hash = stats.hash;
    this.debug_last_frame_log_ms = now;

    const message = "virtio-gpu: frame resource=" + resource.id +
        " size=" + resource.width + "x" + resource.height +
        " scanout=" + scanout.rect.width + "x" + scanout.rect.height +
        " samples=" + stats.samples +
        " nonblack=" + stats.nonblack + "/" + stats.samples +
        " hash=0x" + stats.hash.toString(16) +
        " tl=" + stats.top_left +
        " center=" + stats.center +
        " br=" + stats.bottom_right;

    if(LOG_LEVEL & LOG_VIRTIO)
    {
        dbg_log(message, LOG_VIRTIO);
    }
    else
    {
        console.log(message);
    }
};

VirtioGPU.prototype.cmd_get_display_info = function(hdr)
{
    const buf = new ArrayBuffer(24 + 24 * VIRTIO_GPU_MAX_SCANOUTS);
    const view = new DataView(buf);
    const resp_flags = hdr.flags & VIRTIO_GPU_FLAG_FENCE;

    view.setUint32(0, VIRTIO_GPU_RESP_OK_DISPLAY_INFO, true);
    view.setUint32(4, resp_flags, true);
    write_le64(view, 8, hdr.fence_id);
    view.setUint32(16, hdr.ctx_id, true);
    view.setUint32(20, 0, true);

    const base = 24;

    view.setUint32(base + 0, 0, true);
    view.setUint32(base + 4, 0, true);
    view.setUint32(base + 8, 1024, true);
    view.setUint32(base + 12, 768, true);
    view.setUint32(base + 16, 1, true);
    view.setUint32(base + 20, 0, true);

    return new Uint8Array(buf);
};

VirtioGPU.prototype.cmd_resource_create_2d = function(view, hdr)
{
    const resource_id = view.getUint32(24, true);
    const format = view.getUint32(28, true);
    const width = view.getUint32(32, true);
    const height = view.getUint32(36, true);

    if(width === 0 || height === 0)
    {
        console.warn("virtio-gpu: invalid resource size", width, height);
        return this.resp_err(hdr);
    }

    const max_size = 4096;
    if(width > max_size || height > max_size)
    {
        console.warn("virtio-gpu: resource too large", width, height);
        return this.resp_err(hdr);
    }

    if(format !== VIRTIO_GPU_FORMAT_B8G8R8A8_UNORM &&
        format !== VIRTIO_GPU_FORMAT_B8G8R8X8_UNORM)
    {
        console.warn("virtio-gpu: unsupported format", format);
        return this.resp_err(hdr);
    }

    this.resources.set(resource_id, {
        id: resource_id,
        width,
        height,
        format,
        backing: [],
        backing_size: 0,
        host_buffer: null,
    });

    return this.resp_ok_nodata(hdr);
};

VirtioGPU.prototype.cmd_attach_backing = function(view, hdr)
{
    const resource_id = view.getUint32(24, true);
    const nr_entries = view.getUint32(28, true);
    const resource = this.resources.get(resource_id);

    if(!resource)
    {
        console.warn("virtio-gpu: resource not found for attach_backing", resource_id);
        return this.resp_err(hdr);
    }

    const backing = [];
    let backing_size = 0;
    let offset = 32;

    for(let i = 0; i < nr_entries; i++)
    {
        const addr = read_le64(view, offset);
        const length = view.getUint32(offset + 8, true);

        backing.push({ addr, length });
        backing_size += length;
        offset += 16;
    }

    resource.backing = backing;
    resource.backing_size = backing_size;

    return this.resp_ok_nodata(hdr);
};

VirtioGPU.prototype.cmd_detach_backing = function(view, hdr)
{
    const resource_id = view.getUint32(24, true);
    const resource = this.resources.get(resource_id);

    if(!resource)
    {
        return this.resp_err(hdr);
    }

    resource.backing = [];
    resource.backing_size = 0;
    resource.host_buffer = null;

    return this.resp_ok_nodata(hdr);
};

VirtioGPU.prototype.cmd_resource_unref = function(view, hdr)
{
    const resource_id = view.getUint32(24, true);

    if(!this.resources.has(resource_id))
    {
        return this.resp_ok_nodata(hdr);
    }

    this.resources.delete(resource_id);

    for(const [scanout_id, scanout] of this.scanouts)
    {
        if(scanout.resource_id === resource_id)
        {
            this.scanouts.delete(scanout_id);
        }
    }

    if(this.cursor.resource_id === resource_id)
    {
        this.cursor.visible = false;
        this.cursor.resource_id = 0;
    }

    if(!this.scanouts.size)
    {
        this.display_has_content = false;
    }

    this.update_display_visibility();

    return this.resp_ok_nodata(hdr);
};

VirtioGPU.prototype.cmd_set_scanout = function(view, hdr)
{
    const r_x = view.getUint32(24, true);
    const r_y = view.getUint32(28, true);
    const r_width = view.getUint32(32, true);
    const r_height = view.getUint32(36, true);
    const scanout_id = view.getUint32(40, true);
    const resource_id = view.getUint32(44, true);

    if(resource_id === 0)
    {
        this.scanouts.delete(scanout_id);
        if(!this.scanouts.size)
        {
            this.display_has_content = false;
        }
        this.update_display_visibility();
        return this.resp_ok_nodata(hdr);
    }

    const resource = this.resources.get(resource_id);
    if(!resource)
    {
        console.warn("virtio-gpu: resource not found for set_scanout", resource_id);
        return this.resp_err(hdr);
    }

    if(r_x + r_width > resource.width || r_y + r_height > resource.height ||
        r_width === 0 || r_height === 0)
    {
        console.warn("virtio-gpu: invalid scanout rectangle", { r_x, r_y, r_width, r_height });
        return this.resp_err(hdr);
    }

    this.scanouts.set(scanout_id, {
        resource_id,
        rect: {
            x: r_x,
            y: r_y,
            width: r_width,
            height: r_height,
        },
    });

    this.ensure_display_surface(r_width, r_height);
    this.update_display_visibility();

    return this.resp_ok_nodata(hdr);
};

VirtioGPU.prototype.cmd_transfer_to_host_2d = function(view, hdr)
{
    const base = 24;
    const x = view.getUint32(base + 0, true);
    const y = view.getUint32(base + 4, true);
    const width = view.getUint32(base + 8, true);
    const height = view.getUint32(base + 12, true);
    const offset = Number(read_le64(view, base + 16));
    const resource_id = view.getUint32(base + 24, true);
    const resource = this.resources.get(resource_id);

    if(!resource)
    {
        console.warn("virtio-gpu: invalid transfer resource", resource_id);
        return this.resp_err(hdr);
    }

    if(!resource.backing || resource.backing.length === 0)
    {
        console.warn("virtio-gpu: transfer resource has no backing");
        return this.resp_err(hdr);
    }

    if(x + width > resource.width || y + height > resource.height)
    {
        console.warn("virtio-gpu: transfer rectangle out of bounds", { x, y, width, height });
        return this.resp_err(hdr);
    }

    const bpp = 4;
    const stride = resource.width * bpp;

    if(!resource.host_buffer)
    {
        resource.host_buffer = new Uint8Array(resource.width * resource.height * bpp);
    }

    for(let row = 0; row < height; row++)
    {
        const src_offset = offset + row * stride;
        const dst_row = ((y + row) * resource.width + x) * bpp;
        const src_slice = this.backing_read(resource, src_offset, width * bpp);

        resource.host_buffer.set(src_slice, dst_row);
    }

    return this.resp_ok_nodata(hdr);
};

VirtioGPU.prototype.cmd_resource_flush = function(view, hdr)
{
    const base = 24;
    const x = view.getUint32(base + 0, true);
    const y = view.getUint32(base + 4, true);
    const width = view.getUint32(base + 8, true);
    const height = view.getUint32(base + 12, true);
    const resource_id = view.getUint32(base + 16, true);
    const resource = this.resources.get(resource_id);

    if(!resource)
    {
        console.warn("virtio-gpu: invalid flush resource", resource_id);
        return this.resp_err(hdr);
    }

    if(!resource.host_buffer || !this.scanouts.size)
    {
        return this.resp_ok_nodata(hdr);
    }

    if(x + width > resource.width || y + height > resource.height)
    {
        console.warn("virtio-gpu: flush rectangle out of bounds", { x, y, width, height });
        return this.resp_err(hdr);
    }

    const scanout = this.find_scanout_for_resource(resource_id);
    if(!scanout)
    {
        return this.resp_ok_nodata(hdr);
    }

    this.ensure_display_surface(scanout.rect.width, scanout.rect.height);
    if(!this.display_image_data)
    {
        return this.resp_ok_nodata(hdr);
    }

    this.copy_scanout_to_display(resource, scanout);
    this.debug_log_frame_stats(resource, scanout);
    this.display_has_content = true;
    this.update_display_visibility();
    this.present_display_surface();

    return this.resp_ok_nodata(hdr);
};

VirtioGPU.prototype.find_scanout_for_resource = function(resource_id)
{
    for(const scanout of this.scanouts.values())
    {
        if(scanout.resource_id === resource_id)
        {
            return scanout;
        }
    }

    return null;
};

VirtioGPU.prototype.copy_scanout_to_display = function(resource, scanout)
{
    const scanout_rect = scanout.rect;
    const src = resource.host_buffer;
    const dst = this.display_image_data.data;

    for(let row = 0; row < scanout_rect.height; row++)
    {
        const src_row_byte = ((scanout_rect.y + row) * resource.width + scanout_rect.x) * 4;
        const dst_row_byte = row * scanout_rect.width * 4;

        for(let col = 0; col < scanout_rect.width; col++)
        {
            const src_i = src_row_byte + col * 4;
            const dst_i = dst_row_byte + col * 4;

            const b = src[src_i + 0];
            const g = src[src_i + 1];
            const r = src[src_i + 2];

            dst[dst_i + 0] = r;
            dst[dst_i + 1] = g;
            dst[dst_i + 2] = b;
            dst[dst_i + 3] = 255;
        }
    }
};

VirtioGPU.prototype.backing_read = function(resource, offset, length)
{
    let remaining = length;
    let off = offset;
    const result = new Uint8Array(length);
    let dst_pos = 0;

    for(const entry of resource.backing)
    {
        const entry_len = entry.length;

        if(off >= entry_len)
        {
            off -= entry_len;
            continue;
        }

        const start = Number(entry.addr) + off;
        const copy_len = Math.min(entry_len - off, remaining);
        const memory_size = this.virtio.cpu.memory_size[0];

        if(start < 0 || start + copy_len > memory_size)
        {
            console.warn("virtio-gpu: backing read address out of bounds", {
                start,
                copy_len,
                memory_size,
            });
            break;
        }

        const slice = this.virtio.cpu.read_blob(start, copy_len);
        result.set(slice, dst_pos);

        dst_pos += copy_len;
        remaining -= copy_len;
        off = 0;

        if(remaining === 0)
        {
            break;
        }
    }

    if(remaining !== 0)
    {
        console.warn("virtio-gpu: backing read overflow", { offset, length });
    }

    return result;
};

VirtioGPU.prototype.claim_vga_provider = function()
{
    if(!this.display_width || !this.display_height)
    {
        return false;
    }

    if(!this.vga_provider_registered)
    {
        this.vga.set_external_graphics_provider(this.vga_provider);
        this.vga_provider_registered = true;
    }

    const need_mode_set =
        this.claimed_width !== this.display_width ||
        this.claimed_height !== this.display_height;

    this.vga.screen.set_mode(true);

    if(need_mode_set)
    {
        this.vga.screen.set_size_graphical(
            this.display_width,
            this.display_height,
            this.display_width,
            this.display_height
        );

        this.bus.send("screen-set-size", [
            this.display_width,
            this.display_height,
            32,
        ]);

        this.claimed_width = this.display_width;
        this.claimed_height = this.display_height;
    }

    return true;
};

VirtioGPU.prototype.release_vga_provider = function()
{
    if(!this.vga_provider_registered)
    {
        return;
    }
    
    this.vga.clear_external_graphics_provider(this.vga_provider);
    this.vga_provider_registered = false;
    this.claimed_width = 0;
    this.claimed_height = 0;
    this.restore_legacy_screen();
};

VirtioGPU.prototype.restore_legacy_screen = function()
{
    this.vga.screen.set_mode(this.vga.graphical_mode);

    if(this.vga.graphical_mode)
    {
        this.vga.screen.set_size_graphical(
            this.vga.screen_width,
            this.vga.screen_height,
            this.vga.virtual_width || this.vga.screen_width,
            this.vga.virtual_height || this.vga.screen_height
        );
        this.vga.complete_redraw();
        this.vga.screen_fill_buffer();
        this.bus.send("screen-set-size", [this.vga.screen_width, this.vga.screen_height, infer_vga_bpp(this.vga)]);
    }
    else
    {
        this.vga.screen.set_size_text(this.vga.max_cols, this.vga.max_rows);
        this.vga.complete_redraw();
        this.bus.send("screen-set-size", [this.vga.max_cols, this.vga.max_rows, 0]);
    }
};

VirtioGPU.prototype.ensure_display_surface = function(width, height)
{
    if(!width || !height || typeof ImageData === "undefined")
    {
        return;
    }

    if(this.display_width === width &&
        this.display_height === height &&
        this.display_image_data &&
        this.display_present_image_data)
    {
        return;
    }

    this.display_width = width;
    this.display_height = height;
    this.display_image_data = new ImageData(width, height);
    this.display_present_image_data = new ImageData(width, height);
};

VirtioGPU.prototype.update_display_visibility = function()
{
    const should_show_display = this.scanouts.size > 0 && this.display_has_content;

    if(should_show_display)
    {
        this.claim_vga_provider();
    }
    else
    {
        this.release_vga_provider();
    }
};

VirtioGPU.prototype.present_display_surface = function()
{
    if(!this.display_has_content || !this.get_present_image_data())
    {
        return;
    }

    if(!this.claim_vga_provider())
    {
        return;
    }

    this.vga.screen_fill_buffer();
};

VirtioGPU.prototype.get_vga_provider_frame = function()
{
    const image_data = this.get_present_image_data();
    if(!image_data)
    {
        return null;
    }

    return {
        image_data,
        screen_x: 0,
        screen_y: 0,
        buffer_x: 0,
        buffer_y: 0,
        buffer_width: this.display_width,
        buffer_height: this.display_height,
    };
};

VirtioGPU.prototype.get_present_image_data = function()
{
    if(!this.display_image_data)
    {
        return null;
    }

    const cursor_resource = this.cursor.visible ?
        this.resources.get(this.cursor.resource_id) :
        null;

    if(!cursor_resource || !cursor_resource.host_buffer)
    {
        return this.display_image_data;
    }

    const present = this.display_present_image_data || this.display_image_data;
    if(present !== this.display_image_data)
    {
        present.data.set(this.display_image_data.data);
    }

    this.composite_cursor(present, cursor_resource);

    return present;
};

VirtioGPU.prototype.composite_cursor = function(image_data, cursor_resource)
{
    const dst = image_data.data;
    const src = cursor_resource.host_buffer;
    const width = Math.min(cursor_resource.width, 64);
    const height = Math.min(cursor_resource.height, 64);
    const dst_width = this.display_width;
    const dst_height = this.display_height;
    const left = this.cursor.x - this.cursor.hot_x;
    const top = this.cursor.y - this.cursor.hot_y;

    for(let cy = 0; cy < height; cy++)
    {
        const dy = top + cy;
        if(dy < 0 || dy >= dst_height)
        {
            continue;
        }

        for(let cx = 0; cx < width; cx++)
        {
            const dx = left + cx;
            if(dx < 0 || dx >= dst_width)
            {
                continue;
            }

            const src_i = (cy * cursor_resource.width + cx) * 4;
            const b = src[src_i + 0];
            const g = src[src_i + 1];
            const r = src[src_i + 2];
            let a = src[src_i + 3];

            // Linux cursors may arrive as X8 resources while still carrying a
            // useful alpha byte in the unused channel. Treating all X8 pixels as
            // opaque turns transparent cursor padding into a black square.
            if(cursor_resource.format === VIRTIO_GPU_FORMAT_B8G8R8X8_UNORM &&
                a === 0 &&
                (r !== 0 || g !== 0 || b !== 0))
            {
                a = 255;
            }

            if(a === 0)
            {
                continue;
            }

            const dst_i = (dy * dst_width + dx) * 4;
            if(a === 255)
            {
                dst[dst_i + 0] = r;
                dst[dst_i + 1] = g;
                dst[dst_i + 2] = b;
                dst[dst_i + 3] = 255;
                continue;
            }

            const inv_a = 255 - a;
            dst[dst_i + 0] = (r * a + dst[dst_i + 0] * inv_a + 127) / 255 | 0;
            dst[dst_i + 1] = (g * a + dst[dst_i + 1] * inv_a + 127) / 255 | 0;
            dst[dst_i + 2] = (b * a + dst[dst_i + 2] * inv_a + 127) / 255 | 0;
            dst[dst_i + 3] = 255;
        }
    }
};

VirtioGPU.prototype.resp_ok_nodata = function(hdr)
{
    const buf = new ArrayBuffer(24);
    const view = new DataView(buf);
    const resp_flags = hdr.flags & VIRTIO_GPU_FLAG_FENCE;

    view.setUint32(0, VIRTIO_GPU_RESP_OK_NODATA, true);
    view.setUint32(4, resp_flags, true);
    write_le64(view, 8, hdr.fence_id);
    view.setUint32(16, hdr.ctx_id, true);
    view.setUint32(20, 0, true);

    return new Uint8Array(buf);
};

VirtioGPU.prototype.resp_err = function(hdr)
{
    const buf = new ArrayBuffer(24);
    const view = new DataView(buf);
    const resp_flags = hdr.flags & VIRTIO_GPU_FLAG_FENCE;

    view.setUint32(0, VIRTIO_GPU_RESP_ERR_UNSPEC, true);
    view.setUint32(4, resp_flags, true);
    write_le64(view, 8, hdr.fence_id);
    view.setUint32(16, hdr.ctx_id, true);
    view.setUint32(20, 0, true);

    return new Uint8Array(buf);
};

function createGPUConfigStruct(gpu)
{
    return [
        {
            bytes: 4,
            name: "events_read",
            read: () => gpu.events_read,
            write: data => {},
        },
        {
            bytes: 4,
            name: "events_clear",
            read: () => 0,
            write: data => {
                gpu.events_read &= ~data;
            },
        },
        {
            bytes: 4,
            name: "num_scanouts",
            read: () => 1,
            write: data => {},
        },
        {
            bytes: 4,
            name: "num_capsets",
            read: () => 0,
            write: data => {},
        },
    ];
}

function debug_gpu_frame_stats(resource, scanout)
{
    const rect = scanout.rect;
    const src = resource.host_buffer;
    const samples_x = Math.min(32, rect.width);
    const samples_y = Math.min(18, rect.height);
    const step_x = Math.max(1, rect.width / samples_x | 0);
    const step_y = Math.max(1, rect.height / samples_y | 0);

    let samples = 0;
    let nonblack = 0;
    let hash = 0x811c9dc5;

    for(let y = rect.y; y < rect.y + rect.height; y += step_y)
    {
        for(let x = rect.x; x < rect.x + rect.width; x += step_x)
        {
            const i = (y * resource.width + x) * 4;
            const b = src[i + 0];
            const g = src[i + 1];
            const r = src[i + 2];

            if(r || g || b)
            {
                nonblack++;
            }

            hash ^= r;
            hash = Math.imul(hash, 0x01000193) >>> 0;
            hash ^= g;
            hash = Math.imul(hash, 0x01000193) >>> 0;
            hash ^= b;
            hash = Math.imul(hash, 0x01000193) >>> 0;
            samples++;
        }
    }

    return {
        samples,
        nonblack,
        hash,
        top_left: debug_gpu_pixel(resource, rect.x, rect.y),
        center: debug_gpu_pixel(resource, rect.x + (rect.width >> 1), rect.y + (rect.height >> 1)),
        bottom_right: debug_gpu_pixel(resource, rect.x + rect.width - 1, rect.y + rect.height - 1),
    };
}

function debug_gpu_pixel(resource, x, y)
{
    const i = (y * resource.width + x) * 4;
    const src = resource.host_buffer;

    return "#" +
        debug_hex_byte(src[i + 2]) +
        debug_hex_byte(src[i + 1]) +
        debug_hex_byte(src[i + 0]);
}

function debug_hex_byte(value)
{
    return (value | 0).toString(16).padStart(2, "0");
}

function debug_gpu_command_details(view, length, type)
{
    switch(type)
    {
        case VIRTIO_GPU_CMD_RESOURCE_CREATE_2D:
            return "resource=" + debug_u32(view, length, 24) +
                " format=" + debug_u32(view, length, 28) +
                " size=" + debug_u32(view, length, 32) + "x" + debug_u32(view, length, 36);

        case VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING:
            return "resource=" + debug_u32(view, length, 24) +
                " entries=" + debug_u32(view, length, 28);

        case VIRTIO_GPU_CMD_RESOURCE_DETACH_BACKING:
        case VIRTIO_GPU_CMD_RESOURCE_UNREF:
            return "resource=" + debug_u32(view, length, 24);

        case VIRTIO_GPU_CMD_SET_SCANOUT:
            return "scanout=" + debug_u32(view, length, 40) +
                " resource=" + debug_u32(view, length, 44) +
                " rect=" + debug_rect(view, length, 24);

        case VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D:
            return "resource=" + debug_u32(view, length, 48) +
                " rect=" + debug_rect(view, length, 24) +
                " offset=" + debug_u64(view, length, 40);

        case VIRTIO_GPU_CMD_RESOURCE_FLUSH:
            return "resource=" + debug_u32(view, length, 40) +
                " rect=" + debug_rect(view, length, 24);

        case VIRTIO_GPU_CMD_UPDATE_CURSOR:
            return "scanout=" + debug_u32(view, length, 24) +
                " pos=" + debug_u32(view, length, 28) + "," + debug_u32(view, length, 32) +
                " resource=" + debug_u32(view, length, 40) +
                " hot=" + debug_u32(view, length, 44) + "," + debug_u32(view, length, 48);

        case VIRTIO_GPU_CMD_MOVE_CURSOR:
            return "scanout=" + debug_u32(view, length, 24) +
                " pos=" + debug_u32(view, length, 28) + "," + debug_u32(view, length, 32);

        default:
            return "";
    }
}

function debug_rect(view, length, offset)
{
    return debug_u32(view, length, offset) + "," +
        debug_u32(view, length, offset + 4) + " " +
        debug_u32(view, length, offset + 8) + "x" +
        debug_u32(view, length, offset + 12);
}

function debug_u32(view, length, offset)
{
    return length >= offset + 4 ? view.getUint32(offset, true) : "?";
}

function debug_u64(view, length, offset)
{
    return length >= offset + 8 ? read_le64(view, offset).toString() : "?";
}

function parse_ctrl_hdr(view, offset)
{
    return {
        type: view.getUint32(offset, true),
        flags: view.getUint32(offset + 4, true),
        fence_id: read_le64(view, offset + 8),
        ctx_id: view.getUint32(offset + 16, true),
    };
}

function read_le64(view, offset)
{
    const lo = view.getUint32(offset, true);
    const hi = view.getUint32(offset + 4, true);
    return (BigInt(hi) << 32n) | BigInt(lo);
}

function write_le64(view, offset, value)
{
    const big_value = BigInt(value);
    view.setUint32(offset, Number(big_value & 0xffffffffn), true);
    view.setUint32(offset + 4, Number((big_value >> 32n) & 0xffffffffn), true);
}

function infer_vga_bpp(vga)
{
    if(vga.svga_enabled)
    {
        return vga.svga_bpp;
    }

    if(vga.attribute_mode & 0x40)
    {
        return 8;
    }

    if(vga.attribute_mode & 0x2)
    {
        return 1;
    }

    return 4;
}
