"use strict";
const assert = require("node:assert/strict");
require("../../src/browser/glbridge/v86_network_bridge.js");
const { installV86GLGraphicsAdapter } = require("../../src/browser/glbridge/graphics_adapter.js");

function fixture() {
    const listeners = new Map(), domListeners = new Set(), observers = [], frames = new Map();
    const style = () => ({ setProperty(key, value) { this[key] = value; } });
    const screen = { width: 640, height: 480,
        rect: { left: 148, top: 102, width: 1280, height: 960 },
        getBoundingClientRect() { return this.rect; } };
    const win = {
        getComputedStyle() { return { position: "static" }; },
        requestAnimationFrame(fn) { frames.set(1, fn); return 1; },
        cancelAnimationFrame(id) { frames.delete(id); },
        addEventListener(name) { domListeners.add(name); },
        removeEventListener(name) { domListeners.delete(name); },
        ResizeObserver: class {
            constructor(fn) { this.fn = fn; observers.push(this); }
            observe() {}
            disconnect() { this.disconnected = true; }
        },
    };
    const doc = { defaultView: win, addEventListener: win.addEventListener,
        removeEventListener: win.removeEventListener,
        createElement() { return { style: style(), width: 1, height: 1, setAttribute() {} }; } };
    const container = { ownerDocument: doc, style: { position: "" },
        offsetWidth: 800, offsetHeight: 600, clientLeft: 4, clientTop: 6,
        scrollLeft: 10, scrollTop: 20, children: [screen],
        getBoundingClientRect() { return { left: 100, top: 50, width: 1600, height: 1200 }; },
        getElementsByTagName() { return this.children; },
        appendChild(child) { this.children.push(child); child.parentElement = this; },
        removeChild(child) { this.children.splice(this.children.indexOf(child), 1); child.parentElement = null; },
    };
    let graphical = true, destroyed = 0, unconfigured = 0, terminated = 0;
    const host = { device: { destroy() { destroyed++; } },
        context: { unconfigure() { unconfigured++; } }, deviceLostHandlers: new Set() };
    const gpuResets = [];
    global.V86GPUHost = { reset(canvas) { gpuResets.push(canvas); } };
    const executor = () => ({ host, initialize() { return Promise.resolve(); }, work: Promise.resolve() });
    const emulator = {
        add_listener(name, fn) { listeners.set(name, fn); },
        remove_listener(name, fn) { assert.equal(listeners.get(name), fn); listeners.delete(name); },
    };
    const options = { container, screenCanvas: screen, managedState: true,
        isGraphical: () => graphical, glExecutor: executor(), d3d8Executor: executor(), d3d9Executor: executor() };
    options.d3d9Executor.shaderWorker = { terminate() { terminated++; } };
    return { emulator, options, container, screen, listeners, domListeners, frames, observers,
        setGraphical(value) { graphical = value; },
        stats: () => ({ destroyed, unconfigured, terminated, gpuResets: gpuResets.length }) };
}

(async () => {
    const f = fixture();
    assert.throws(() => installV86GLGraphicsAdapter(f.emulator, { ...f.options, graphicsCanvas: f.screen }), /separate canvases/);
    const bridge = installV86GLGraphicsAdapter(f.emulator, f.options);
    await bridge.ready;
    assert.equal(f.container.children[0], f.screen, "VGA keeps its own canvas");
    assert.equal(f.container.style.position, "relative");
    assert.equal(bridge.canvas.style.display, "none", "boot starts on VGA");
    bridge.glSurface = { x: 50, y: 60, width: 200, height: 100 };
    bridge.showOwner("gl");
    assert.equal(bridge.canvas.style.left, "80px", "ancestor scale, border and scroll are accounted for");
    assert.equal(bridge.canvas.style.top, "100px");
    assert.equal(bridge.canvas.style.width, "200px");
    assert.equal(bridge.canvas.style.pointerEvents, "none", "mouse input reaches VGA/container");
    f.screen.rect.width = 640;
    f.screen.rect.height = 480;
    f.observers[0].fn();
    assert.equal(bridge.canvas.style.width, "100px", "responsive VGA resize scales the overlay");
    assert.equal(bridge.canvas.style.left, "55px");
    bridge.glSurface = { x: -20, y: -10, width: 200, height: 100 };
    bridge.positionCanvas();
    assert.equal(bridge.canvas.style["clip-path"], "inset(10% 0% 0% 10%)", "off-desktop pixels are clipped");
    bridge.glSurface = { x: 400, y: 300, width: 640, height: 480, ddDesktopPrimary: true,
        clipRect: { left: 32, top: 24, right: 608, bottom: 456, baseWidth: 640, baseHeight: 480 } };
    bridge.positionCanvas();
    assert.equal(bridge.canvas.style.left, "30px", "DirectDraw primary stays aligned to the desktop");
    assert.equal(bridge.canvas.style["clip-path"], "inset(5% 5% 5% 5%)");
    const extra = f.container.ownerDocument.createElement("canvas");
    extra.className = "v86gl-d3d9-swapchain-overlay";
    f.container.appendChild(extra);
    bridge.d3d9SwapChainCanvases.set(7, extra);
    bridge.positionD3D9SwapChainCanvas({ swapChain: 7, x: 10, y: 20, width: 100, height: 80 });
    f.screen.rect.width = 1280;
    bridge.screenChanged();
    assert.equal(extra.style.width, "100px", "extra swap chains resize with VGA too");
    bridge.hideOverlayCanvas();
    assert.equal(extra.style.display, "block", "hiding a primary window preserves independent swap-chain windows");
    f.setGraphical(false);
    bridge.screenChanged();
    assert.equal(bridge.canvas.style.display, "none", "text mode hides the overlay");
    assert.equal(extra.style.display, "none");
    f.setGraphical(true);
    bridge.screenChanged();
    assert.equal(bridge.canvas.style.display, "none", "return to VGA graphics waits for a fresh Present");
    const promise = bridge.destroy();
    assert.equal(bridge.destroy(), promise, "destroy is idempotent");
    await promise;
    assert.deepEqual(f.stats(), { destroyed: 1, unconfigured: 1, terminated: 1, gpuResets: 1 });
    assert.equal(f.listeners.size, 0);
    assert.equal(f.domListeners.size, 0);
    assert.equal(f.frames.size, 0);
    assert.equal(f.observers[0].disconnected, true);
    assert.deepEqual(f.container.children, [f.screen]);
    assert.equal(f.container.style.position, "");
    bridge.showOwner("gl");
    assert.equal(bridge.canvas.style.display, "none", "late callbacks cannot resurrect a destroyed overlay");
    console.log("graphics_adapter_test: ok");
})().catch(error => { console.error(error); process.exitCode = 1; });
