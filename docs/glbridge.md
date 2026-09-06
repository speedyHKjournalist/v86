# Virtio graphics on WebGPU

The optional browser graphics bundle contains the host half of the existing
v86gl proxy protocol. Guest DLLs and the `v86gl.sys` driver remain in the
retro-gaming-site repository. The custom virtio transport retains guest opcodes, DMA layouts, response
offsets and the DLL IOCTL ABI, but requires the new virtio driver. See
[transport protocol, migration and build instructions](v86gl-virtio.md).

## Build and embed

For the built-in `index.html` setup page, run `make run` and select
**Enable graphics proxy (WebGPU)** before starting the emulator. This is off
by default; `?graphics_proxy=1` preselects it and also enables it for URL-based
profile boots. The page loads the graphics bundle only when enabled, using
the current manifest revision, and reports loading/initialization failures.
`make run` rebuilds the page bundle and graphics assets before serving them.
Guest proxy DLLs and the `v86gl` driver must still be installed in the guest.

```sh
make build/libv86.js glbridge
make test-glbridge
```

Publish `build/libv86.js`, the matching `build/v86.wasm`, and the complete
`build/glbridge/` directory. The graphics bundle and its two worker resources
must come from the same build. `manifest.json` records a content revision;
use that revision to cache-bust the graphics bundle URL. The worker URL and
persistent shader cache revision are derived from the same revision.

```html
<div id="screen"><div></div><canvas></canvas></div>
<script src="libv86.js"></script>
<script src="glbridge/libv86-webgpu.js?v=BUILD_REVISION"></script>
<script>
const emulator = new V86({
    // BIOS, disk images, memory and other standard v86 options go here.
    wasm_path: "v86.wasm",
    screen: { container: document.getElementById("screen") },
    graphics_adapter: installV86GLGraphicsAdapter,
    graphics_options: {
        onError: error => console.error("Graphics unavailable", error),
    },
});
</script>
```

The factory enables `v86gl_pci` with a 16 MiB batch limit; an explicit
`v86gl_pci: { port, maxBatchBytes }` overrides those defaults. Omitting the
factory retains normal VGA-only behavior and requires no graphics bundle.
GPU initialization failure leaves VGA available and records
`emulator.graphics_adapter.failed`. Serve WebGPU over HTTPS or localhost.

## VGA canvas ownership

The screen adapter selects the VGA **2D** canvas first. Canvases tagged with
`data-v86-graphics` are excluded; `screen.canvas` can select a particular VGA
canvas explicitly. The graphics adapter receives that exact element and
creates a separate, absolutely positioned WebGPU overlay. A supplied
`graphics_options.graphicsCanvas` must be a different canvas in the same
container. The adapter never resizes or reconfigures the VGA canvas.

VGA continues rendering underneath the overlay. Game-window coordinates map
to VGA pixels, including responsive scaling, fullscreen layout, container
scrolling, borders, and axis-aligned ancestor scaling. Window contents clip
to the VGA desktop; DirectDraw desktop-primary clipping is preserved.
ResizeObserver and screen mode/size/scale callbacks update the main and
additional swap-chain canvases. Rotated/skewed CSS ancestors and CSS borders
on the VGA canvas itself are not supported by this coordinate mapping.

The overlay has `pointer-events: none`, so the existing v86 input adapter
continues to receive input. Text-mode transitions and game teardown hide
graphics and expose VGA. `screen_make_screenshot()` composites an active
graphics overlay with VGA; otherwise it keeps the existing VGA/text behavior.
Fullscreen requests target the screen's actual container, independent of ID.
Caller CSS should target `canvas:not([data-v86-graphics])` for VGA-only sizing.

`destroy()` removes listeners, observers and owned canvases, terminates the
shader worker and releases the shared device. Caller-owned canvases remain
hidden in place. Await `destroy()` before reusing a container or canvas.
`restart()` now returns a promise and resets graphics before restarting CPU
execution; await it when sequencing subsequent operations.

## Protocol routing and state

* Untagged OpenGL records use `gl-webgpu/`.
* D9WG (`0xFFE1`) uses `d3d9-webgpu/`. Current D3D8, D3D9 and
  DirectDraw/D3D7 DLLs share this route; `ddraw_ops.js` installs DirectDraw ops.
* Legacy D8WG (`0xFFE0`) still uses `d3d8-webgpu/` for compatibility.

The PCI device now has an internal graphics-state handler pair instead of
requiring the page to replace `get_state`/`set_state`. Checkpoints retain the
graphics state slot 8 and VGS2/version-1 rendering layout. Transport state
now includes virtio queues and arena registration. Old eight-field PCI-device
snapshots require a cold boot with the new driver; VGA-only snapshots without
a graphics device are still supported. Public save/restore and constructor `initial_state` coordinate the
graphics restore before execution resumes. Pause the emulator before a manual
save/restore, as with the existing site workflow.

Checkpoint coverage remains **OpenGL replay plus legacy D8WG resources**.
D9WG resources (including current D3D8 and DDraw/D3D7) are not serialized by
this format. Full D9WG graphics save/restore is a separate feature; moving
the host does not add it. GPU-loss recovery also retains each executor's
existing limitations.

## Source and tests

Runtime source is in `src/browser/glbridge/`. The local CommonJS package scope
preserves the existing Node test exports; the browser bundle is assembled in
explicit dependency order without rewriting rendering algorithms. Host
tests live in `tests/glbridge/`. Guest ABI and cross-repository consistency
tests remain in retro-gaming-site and accept `V86_ROOT` pointing here.

```sh
# A separate temporary Chrome profile and a localhost test server:
node tests/glbridge/gl_multipass_browser_runner.js graphics_vga_browser_test.html
node tests/glbridge/gl_multipass_browser_runner.js
# Other migrated real-GPU fixtures:
node tests/glbridge/gl_multipass_browser_runner.js d3d9_webgpu_browser_test.html
```

Set `GL_CHROME` to select a Chromium executable on another platform. WGSL
compiler tests report skips when `naga` is unavailable. Shader corpus and
performance tools remain separately runnable. Regenerate the GL coverage
report with `node tools/gen_gl_coverage.cjs`.
