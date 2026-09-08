// Optional browser adapter. Load after v86_network_bridge.js and the executors.
(function(global) {
    "use strict";

    function installV86GLGraphicsAdapter(emulator, options) {
        options = options || {};
        const container = options.container;
        const screen = options.screenCanvas;
        if (!container || !screen)
            throw new Error("Graphics adapter requires the v86 VGA canvas and container");
        const doc = container.ownerDocument;
        const win = doc.defaultView;
        const canvas = options.graphicsCanvas || doc.createElement("canvas");
        if (canvas === screen)
            throw new Error("WebGPU and VGA must use separate canvases");
        if (canvas.parentElement && canvas.parentElement !== container)
            throw new Error("Graphics canvas must be in the VGA container");
        canvas.setAttribute("data-v86-graphics", "");
        canvas.style.zIndex = "5";
        const ownedCanvas = !options.graphicsCanvas;
        if (!canvas.parentElement) container.appendChild(canvas);
        const oldPosition = container.style.position;
        const setPosition = win.getComputedStyle(container).position === "static";
        if (setPosition) container.style.position = "relative";

        const bridge = global.installV86GLNetworkBridge(emulator, canvas, options);
        let raf = 0;
        let cleanup;
        bridge.screenChanged = () => {
            if (bridge.destroyed) return;
            if (options.isGraphical && !options.isGraphical()) {
                bridge.hideOverlayCanvas(true);
                bridge.d3d9SwapChainSurfaces.clear();
            }
            bridge.positionCanvas();
            if (!raf) raf = win.requestAnimationFrame(() => {
                raf = 0;
                if (!bridge.destroyed) bridge.positionCanvas();
            });
        };
        const observer = typeof win.ResizeObserver === "function" ?
            new win.ResizeObserver(bridge.screenChanged) : null;
        if (observer) { observer.observe(screen); observer.observe(container); }
        win.addEventListener("resize", bridge.screenChanged);
        // Capture catches scrolling of the container and its ancestors.
        win.addEventListener("scroll", bridge.screenChanged, true);
        doc.addEventListener("fullscreenchange", bridge.screenChanged);

        const executors = () => [bridge.glExecutor, bridge.d3d8Executor, bridge.d3d9Executor].filter(Boolean);
        const initialize = async () => {
            try {
                await Promise.all(executors().map(executor => executor.initialize && executor.initialize()));
            } catch (error) {
                bridge.failed = error;
                bridge.hideOverlayCanvas(true);
                if (typeof options.onError === "function") options.onError(error);
                else console.error("[v86gl] graphics unavailable; VGA remains active", error);
            }
        };
        const disposeExecutors = async () => {
            const old = executors();
            await Promise.allSettled(old.flatMap(executor => [executor.readyPromise,
                executor.checkpointIdle ? executor.checkpointIdle() :
                    executor.idle ? executor.idle() : executor.work]));
            const hosts = new Set(old.map(executor => executor.host).filter(Boolean));
            for (const executor of old) {
                executor.destroyed = true;
                if (executor.shaderCacheSaveTimer !== null && executor.shaderCacheSaveTimer !== undefined) {
                    global.clearTimeout(executor.shaderCacheSaveTimer);
                    executor.shaderCacheSaveTimer = null;
                    await executor.flushPersistentShaderCache();
                }
                if (executor.shaderWorker) executor.shaderWorker.terminate();
                if (executor.shaderWorkerRequests) executor.shaderWorkerRequests.clear();
                if (executor.pending) executor.pending.length = 0;
            }
            for (const host of hosts) {
                if (host.recovering) await host.recovering.catch(() => {});
                host.destroyed = true;
                host.deviceLostHandlers.clear();
                if (host.context && host.context.unconfigure) host.context.unconfigure();
                if (host.device && host.device.destroy) host.device.destroy();
                global.V86GPUHost.reset(host.canvas);
            }
            for (const handle of bridge.d3d9SwapChainCanvases.keys())
                bridge.removeD3D9SwapChainCanvas(handle);
            bridge.glExecutor = bridge.d3d8Executor = bridge.d3d9Executor = null;
        };
        const resetExecutors = async (restoring) => {
            ++bridge.memoryGeneration;
            bridge.suspended = true;
            bridge.restoringState = true;
            bridge.hideOverlayCanvas(true);
            await disposeExecutors();
            await bridge.resetJournal();
            bridge.legacyCheckpoint = new Uint8Array(0);
            bridge.glJournal = [];
            bridge.glJournalBytes = 0;
            bridge.glJournalOverflow = false;
            if (!restoring) bridge.pendingBatches = [];
            bridge.d3d8BatchStreamSeen = bridge.d3d9BatchStreamSeen = false;
            bridge.sharedD3DCanvasConflictReported = false;
            bridge.d3d8OwnerSessionKey = bridge.d3d9OwnerSessionKey = null;
            bridge.glSurface = bridge.emptySurface();
            bridge.d3d8Surface = bridge.emptySurface();
            bridge.d3d9Surface = bridge.emptySurface();
            bridge.failed = null;
            bridge.installGLExecutor();
            bridge.installD3D8Executor();
            bridge.installD3D9Executor();
            bridge.ready = initialize();
            await bridge.ready;
            bridge.restoringState = !!restoring;
            bridge.suspended = false;
            bridge.hideOverlayCanvas(true);
        };
        bridge.reset = () => resetExecutors(false);
        bridge.resetForStateRestore = () => resetExecutors(true);
        bridge.makeScreenshot = () => {
            const hasExtra = Array.from(bridge.d3d9SwapChainCanvases.values())
                .some(canvas => canvas.style.display !== "none");
            if (bridge.destroyed || !bridge.activeOwner && !hasExtra || options.isGraphical && !options.isGraphical())
                return null;
            const output = doc.createElement("canvas");
            output.width = screen.width;
            output.height = screen.height;
            const ctx = output.getContext("2d");
            ctx.drawImage(screen, 0, 0);
            const screenRect = screen.getBoundingClientRect();
            if (!screenRect.width || !screenRect.height) return null;
            const overlays = new Set([bridge.activeOwner && bridge.ownerCanvas(bridge.activeOwner),
                ...bridge.d3d9SwapChainCanvases.values()]);
            for (const overlay of overlays) {
                if (!overlay || overlay.style.display === "none") continue;
                const rect = overlay.getBoundingClientRect();
                const x = (rect.left - screenRect.left) * screen.width / screenRect.width;
                const y = (rect.top - screenRect.top) * screen.height / screenRect.height;
                const w = rect.width * screen.width / screenRect.width;
                const h = rect.height * screen.height / screenRect.height;
                ctx.save();
                const inset = (overlay.style.clipPath || "").match(/[\d.]+(?=%)/g);
                if (inset && inset.length === 4) {
                    const [top, right, bottom, left] = inset.map(Number);
                    ctx.beginPath();
                    ctx.rect(x + left / 100 * w, y + top / 100 * h,
                        Math.max(0, (1 - (left + right) / 100) * w),
                        Math.max(0, (1 - (top + bottom) / 100) * h));
                    ctx.clip();
                }
                ctx.drawImage(overlay, x, y, w, h);
                ctx.restore();
            }
            const image = new win.Image();
            image.src = output.toDataURL("image/png");
            return image;
        };
        bridge.destroy = () => {
            if (cleanup) return cleanup;
            bridge.destroyed = true;
            ++bridge.memoryGeneration;
            bridge.hideOverlayCanvas(true);
            emulator.remove_listener("v86gl-pci-frame", bridge.frameListener);
            emulator.remove_listener("emulator-loaded", bridge.loadedListener);
            if (observer) observer.disconnect();
            if (raf) win.cancelAnimationFrame(raf);
            win.removeEventListener("resize", bridge.screenChanged);
            win.removeEventListener("scroll", bridge.screenChanged, true);
            doc.removeEventListener("fullscreenchange", bridge.screenChanged);
            cleanup = (async () => {
                await bridge.pendingRestore.catch(() => {});
                await disposeExecutors();
                bridge.pendingBatches = [];
                bridge.glJournal = [];
                await bridge.graphicsJournal.destroy();
                bridge.preparedCheckpoint = null;
                bridge.graphicsJournalBytes = 0;
                bridge.legacyCheckpoint = new Uint8Array(0);
                if (ownedCanvas && canvas.parentElement === container) container.removeChild(canvas);
                if (setPosition) container.style.position = oldPosition;
                const pci = bridge.pciStateDevice;
                if (pci && pci.__v86glStateBridge === bridge) {
                    pci.get_state = bridge.originalGetState;
                    pci.set_state = bridge.originalSetState;
                    delete pci.__v86glStateBridge;
                }
            })();
            return cleanup;
        };
        bridge.hideOverlayCanvas(true);
        bridge.ready = initialize();
        return bridge;
    }

    global.installV86GLGraphicsAdapter = installV86GLGraphicsAdapter;
    if (typeof module !== "undefined" && module.exports)
        module.exports = { installV86GLGraphicsAdapter };
})(typeof globalThis !== "undefined" ? globalThis : this);
