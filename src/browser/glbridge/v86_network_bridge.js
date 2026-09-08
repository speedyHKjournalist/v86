// v86 PCI DMA graphics router.
//
// OpenGL records are executed directly by gl-webgpu/gl_executor.js. D3D8 and
// D3D9 keep their tagged envelopes on the same v86gl.sys transport, but there
// is deliberately no GL4ES/WebGL decoder or fallback in this file.

(function(global) {
    "use strict";

    const GraphicsJournal = global.V86GraphicsJournal ||
        (typeof require === "function" ? require("./graphics_journal.js").GraphicsJournal : null);
    const V86GL_BRIDGE_VERSION = "gl-webgpu-only-v1-20260824";
    const CTRL_D3D8_BATCH = 0xFFE0;
    const CTRL_D3D9_BATCH = 0xFFE1;
    const EXTENDED_RECORD_SIZE = 0xFFFF;
    const PCI_STATE_GRAPHICS_INDEX = 8;
    const CHECKPOINT_MAGIC = 0x32534756; // "VGS2"
    const CHECKPOINT_VERSION = 2;
    const JOURNAL_ENTRY_BYTES = 32;
    const CHECKPOINT_HEADER_BYTES = 32;
    const DEFAULT_JOURNAL_MEMORY_BYTES = 64 * 1024 * 1024;

    // Version 3 compresses/spools every accepted batch, including queries and Presents.
    // Readback writers are disabled during replay; object creation and query
    // begin/end still run, so guest handles and unfinished frames survive.

    function u16(bytes, offset) {
        return bytes[offset] | bytes[offset + 1] << 8;
    }

    function u32(bytes, offset) {
        return (bytes[offset] | bytes[offset + 1] << 8 |
            bytes[offset + 2] << 16 | bytes[offset + 3] << 24) >>> 0;
    }

    function asBytes(value) {
        if (value instanceof Uint8Array) return value;
        if (ArrayBuffer.isView(value))
            return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
        if (value instanceof ArrayBuffer) return new Uint8Array(value);
        return new Uint8Array(value || []);
    }

    function ownedBytes(value) {
        return asBytes(value).slice();
    }

    function styleSet(canvas, name, value, priority) {
        if (!canvas || !canvas.style) return;
        if (typeof canvas.style.setProperty === "function")
            canvas.style.setProperty(name, value, priority || "");
        else canvas.style[name] = value;
    }

    class V86GLNetworkBridge {
        constructor(emulator, canvas, options) {
            this.emulator = emulator;
            this.options = options || {};
            const shared = this.options.graphicsCanvas ||
                this.options.d3dCanvas || canvas;
            this.canvas = shared;
            this.glCanvas = this.options.glCanvas || shared;
            this.d3d8Canvas = this.options.d3d8Canvas || shared;
            this.d3d9Canvas = this.options.d3d9Canvas || shared;
            this.container = this.options.container || shared && shared.parentElement;
            this.screenCanvas = this.options.screenCanvas || this.findScreenCanvas();
            if (this.screenCanvas && [this.glCanvas, this.d3d8Canvas, this.d3d9Canvas].includes(this.screenCanvas))
                throw new Error("WebGPU requires a canvas separate from the VGA 2D canvas");
            this.destroyed = false;
            this.memoryGeneration = 0;

            this.glExecutor = null;
            this.d3d8Executor = null;
            this.d3d9Executor = null;
            this.glSurface = this.emptySurface();
            this.d3d8Surface = this.emptySurface();
            this.d3d9Surface = this.emptySurface();
            this.activeOwner = null;
            this.d3d8BatchStreamSeen = false;
            this.d3d9BatchStreamSeen = false;
            this.d3d8OwnerSessionKey = null;
            this.d3d9OwnerSessionKey = null;
            this.sharedD3DCanvasConflictReported = false;
            this.d3d9SwapChainCanvases = new Map();
            this.d3d9SwapChainSurfaces = new Map();

            this.journalBudget = this.options.graphicsJournalMemoryBytes ??
                this.options.maxGraphicsJournalBytes ?? this.options.maxGLJournalBytes ?? DEFAULT_JOURNAL_MEMORY_BYTES;
            this.graphicsJournal = new GraphicsJournal({ budget: this.journalBudget });
            this.preparedCheckpoint = null;
            this.graphicsJournalBytes = 0;
            this.legacyCheckpoint = new Uint8Array(0);
            this.glJournal = [];
            this.glJournalBytes = 0;
            this.glJournalOverflow = false;
            this.replayingState = false;
            this.restoringState = false;
            this.restorePrepared = false;
            this.restoreSeen = false;
            this.restoreHadCheckpoint = false;
            this.pendingRestore = Promise.resolve();
            this.pendingBatches = [];
            this.pciStateDevice = null;

            this.installGLExecutor();
            this.installD3D8Executor();
            this.installD3D9Executor();

            if (emulator && typeof emulator.add_listener === "function") {
                this.frameListener = event => this.pushPCIBatch(event);
                this.loadedListener = () => this.attachPCIStateHooks();
                emulator.add_listener("v86gl-pci-frame", this.frameListener);
                emulator.add_listener("emulator-loaded", this.loadedListener);
            }
            this.attachPCIStateHooks();
        }

        emptySurface() {
            return { hwnd: 0, x: 0, y: 0, width: 0, height: 0,
                     displayWidth: 0, displayHeight: 0, visible: false };
        }

        installGLExecutor() {
            if (this.options.glExecutor) {
                this.glExecutor = this.options.glExecutor;
                return;
            }
            const install = this.options.installGLWebGPUExecutor ||
                global.installGLWebGPUExecutor;
            if (!this.glCanvas || typeof install !== "function") {
                console.error("[gl-webgpu] executor unavailable; load " +
                    "gl_executor.js and provide the shared graphics canvas");
                return;
            }
            const executorOptions = { ...(this.options.gl || {}) };
            const userSurface = executorOptions.onSurface;
            executorOptions.onSurface = (surface, reason) => {
                this.glSurface = { ...this.glSurface, ...surface };
                if (reason === "hide" || surface.visible === false) {
                    if (this.activeOwner === "gl") this.hideOverlayCanvas();
                } else {
                    this.showOwner("gl");
                }
                if (typeof userSurface === "function") userSurface(surface, reason);
            };
            executorOptions.writeGuestMemory = (offsetInBatch, data, metadata) => {
                if (this.destroyed || this.suspended || this.restoringState || this.replayingState) return;
                if (metadata && metadata.graphicsGeneration !== undefined &&
                    metadata.graphicsGeneration !== this.memoryGeneration) return;
                if (!metadata || metadata.batchAddress === undefined) return;
                if (typeof metadata.writeGuestMemory === "function")
                    return metadata.writeGuestMemory(32 + offsetInBatch, asBytes(data));
                if (!this.emulator ||
                        typeof this.emulator.write_memory !== "function") return;
                this.emulator.write_memory(asBytes(data),
                    (metadata.batchAddress + offsetInBatch) >>> 0);
            };
            this.glExecutor = install(this.glCanvas, executorOptions);
        }

        installD3D8Executor() {
            if (this.options.d3d8Executor) {
                this.d3d8Executor = this.options.d3d8Executor;
                return;
            }
            const install = this.options.installD3D8WebGPUExecutor ||
                global.installD3D8WebGPUExecutor;
            if (!this.d3d8Canvas || typeof install !== "function") return;
            const opts = { ...(this.options.d3d8 || {}) };
            const userSurface = opts.onSurface;
            const userPresent = opts.onPresent;
            const userDestroy = opts.onDestroy;
            opts.onSurface = (surface, reason) => {
                const session = surface.sessionKey || null;
                if (reason === "hide" || surface.visible === false) {
                    if (!session || session === this.d3d8OwnerSessionKey) {
                        this.d3d8Surface = { ...this.d3d8Surface, ...surface };
                        if (this.activeOwner === "d3d8") this.hideOverlayCanvas();
                    }
                } else {
                    this.d3d8OwnerSessionKey = session;
                    this.d3d8Surface = { ...this.d3d8Surface, ...surface };
                    this.positionOwner("d3d8", false);
                }
                if (typeof userSurface === "function") userSurface(surface, reason);
            };
            opts.onPresent = (surface, stats) => {
                this.d3d8OwnerSessionKey = surface.sessionKey || null;
                this.d3d8Surface = { ...this.d3d8Surface, ...surface };
                if (surface.visible === false) this.hideOverlayCanvas();
                else this.showOwner("d3d8");
                if (typeof userPresent === "function") userPresent(surface, stats);
            };
            opts.onDestroy = (surface, reason) => {
                const session = surface.sessionKey || null;
                if (!session || session === this.d3d8OwnerSessionKey) {
                    this.d3d8OwnerSessionKey = null;
                    this.d3d8Surface = { ...this.d3d8Surface, ...surface,
                        visible: false };
                    if (this.activeOwner === "d3d8") this.hideOverlayCanvas();
                }
                if (typeof userDestroy === "function") userDestroy(surface, reason);
            };
            this.d3d8Executor = install(this.d3d8Canvas, opts);
        }

        installD3D9Executor() {
            if (this.options.d3d9Executor) {
                this.d3d9Executor = this.options.d3d9Executor;
                return;
            }
            const install = this.options.installD3D9WebGPUExecutor ||
                global.installD3D9WebGPUExecutor;
            if (!this.d3d9Canvas || typeof install !== "function") return;
            const opts = { ...(this.options.d3d9 || {}) };
            const userSurface = opts.onSurface;
            const userPresent = opts.onPresent;
            const userDestroy = opts.onDestroy;
            opts.onSurface = (surface, reason) => {
                const session = surface.sessionKey || null;
                if (reason === "hide" || surface.visible === false) {
                    if (!session || session === this.d3d9OwnerSessionKey) {
                        this.d3d9Surface = { ...this.d3d9Surface, ...surface };
                        if (this.activeOwner === "d3d9")
                            this.hideOverlayCanvas();
                        else
                            this.styleOverlayCanvas(this.d3d9Canvas,
                                0, 0, 0, 0, false);
                    }
                } else {
                    /* A CREATE/RESET only declares where a process would
                     * present; it does not make that process the visible
                     * owner. Capability helpers interleave their batches with
                     * games and dxdiag, so stealing ownership here made the
                     * live canvas disappear until the helper's (often absent)
                     * Present. Ownership changes only after a successful
                     * onPresent below. */
                    const hasOwner = this.d3d9OwnerSessionKey !== null;
                    const belongsToOwner = hasOwner &&
                        session === this.d3d9OwnerSessionKey;
                    const legacyOwner = !hasOwner && !session &&
                        this.activeOwner === "d3d9";
                    if (!hasOwner || belongsToOwner) {
                        this.d3d9Surface = { ...this.d3d9Surface, ...surface };
                        // Only a successful Present may put the shared canvas
                        // on screen.  In particular, DirectDraw sends a
                        // display-mode report while tearing down 3DMark 2000;
                        // treating that geometry report as visibility showed
                        // the retained (black) canvas after the primary had
                        // already told us to hide it.
                        this.positionOwner("d3d9",
                            this.activeOwner === "d3d9" &&
                            (belongsToOwner || legacyOwner));
                    }
                }
                if (typeof userSurface === "function") userSurface(surface, reason);
            };
            opts.onPresent = (surface, stats) => {
                this.d3d9OwnerSessionKey = surface.sessionKey || null;
                this.d3d9Surface = { ...this.d3d9Surface, ...surface };
                if (surface.visible === false) this.hideOverlayCanvas();
                else this.showOwner("d3d9");
                if (typeof userPresent === "function") userPresent(surface, stats);
            };
            opts.onDestroy = (surface, reason) => {
                const session = surface.sessionKey || null;
                // The session check stops a helper process -- dxdiag, a
                // capability probe -- from tearing down a running game's
                // canvas, and that has to keep working.
                //
                // What it cannot answer is the orphaned case: the canvas is
                // still showing frames while no session claims it any more,
                // which is where an earlier destroy in the same session has
                // already cleared the owner. Nothing can take it down after
                // that, so an exiting process leaves a picture of itself over
                // the guest's desktop with nothing left running to remove it.
                // A session ending is a safe moment to reclaim that: there is
                // no owner left to protect.
                const orphaned = this.d3d9OwnerSessionKey === null &&
                    this.activeOwner === "d3d9";
                if (!session || session === this.d3d9OwnerSessionKey ||
                        (reason === "session-end" && orphaned)) {
                    this.d3d9OwnerSessionKey = null;
                    this.d3d9Surface = { ...this.d3d9Surface, ...surface,
                        visible: false };
                    if (this.activeOwner === "d3d9")
                        this.hideOverlayCanvas();
                    else
                        this.styleOverlayCanvas(this.d3d9Canvas,
                            0, 0, 0, 0, false);
                }
                if (typeof userDestroy === "function") userDestroy(surface, reason);
            };
            const userCreateSwapChainCanvas = opts.createSwapChainCanvas;
            opts.createSwapChainCanvas = surface => {
                if (this.destroyed || this.suspended) return null;
                if (typeof userCreateSwapChainCanvas === "function")
                    return userCreateSwapChainCanvas(surface);
                if (typeof document === "undefined" || !this.container) return null;
                const extra = document.createElement("canvas");
                extra.className = "v86gl-d3d9-swapchain-overlay";
                extra.setAttribute("data-v86-graphics", "swapchain");
                extra.style.zIndex = "5";
                extra.width = Math.max(1, surface.width || 1);
                extra.height = Math.max(1, surface.height || 1);
                this.container.appendChild(extra);
                this.d3d9SwapChainCanvases.set(surface.swapChain, extra);
                return extra;
            };
            const userSwapChainSurface = opts.onSwapChainSurface;
            opts.onSwapChainSurface = (surface, reason) => {
                if (reason === "destroy") this.removeD3D9SwapChainCanvas(surface.swapChain);
                else this.positionD3D9SwapChainCanvas(surface);
                if (typeof userSwapChainSurface === "function")
                    userSwapChainSurface(surface, reason);
            };
            this.d3d9Executor = install(this.d3d9Canvas, opts);
        }

        pushPCIBatch(event) {
            if (this.destroyed || this.failed) return;
            event.handled = true;
            const bytes = asBytes(event && event.bytes);
            if (this.restoringState) {
                this.pendingBatches.push({ ...event, bytes: bytes.slice() });
                return;
            }
            if (!this.replayingState) this.recordGraphicsBatch(event, bytes);
            return this.executePCIBatch(event, bytes);
        }

        executePCIBatch(event, bytes) {
            if (this.isEnvelope(bytes, CTRL_D3D8_BATCH))
                return this.pushD3D8PCIBatch(event, bytes);
            if (this.isEnvelope(bytes, CTRL_D3D9_BATCH))
                return this.pushD3D9PCIBatch(event, bytes);
            this.pushGLPCIBatch(event, bytes);
        }

        isEnvelope(bytes, opcode) {
            return bytes.byteLength >= 8 && u16(bytes, 0) === opcode &&
                u16(bytes, 2) === EXTENDED_RECORD_SIZE;
        }

        pushGLPCIBatch(event, bytes) {
            if (!this.glExecutor || typeof this.glExecutor.submit !== "function") {
                console.error("[gl-webgpu] executor unavailable");
                return;
            }
            this.glExecutor.submit(bytes, {
                ...(event.replay ? { replay: true } : {}),
                pciFrameId: event.frameId >>> 0,
                submitCount: event.submitCount >>> 0,
                descriptorCommandCount: event.commandCount >>> 0,
                batchAddress: event.batchAddr === undefined ? undefined :
                    event.batchAddr >>> 0,
                responseBase: event.responseBase,
                writeGuestMemory: event.writeGuestMemory,
                isMemoryValid: event.isMemoryValid,
                graphicsGeneration: this.memoryGeneration,
            });
            if (event.flags & 1) {
                this.glExecutor.onSwapBuffers();
                this.showOwner("gl");
            }
        }

        pushD3D8PCIBatch(event, bytes) {
            if (!this.d3d8Executor ||
                    typeof this.d3d8Executor.submit !== "function") {
                console.error("[d3d8-webgpu] executor unavailable");
                return;
            }
            const payloadBytes = u32(bytes, 4);
            if (payloadBytes !== bytes.byteLength - 8) {
                console.error("[d3d8-webgpu] malformed D8WG envelope");
                return;
            }
            this.warnOnSharedD3DCanvasConflict("d3d8");
            return this.d3d8Executor.submit(bytes.subarray(8), {
                ...(event.replay ? { replay: true } : {}),
                pciFrameId: event.frameId >>> 0,
                submitCount: event.submitCount >>> 0,
                descriptorCommandCount: event.commandCount >>> 0,
            });
        }

        pushD3D9PCIBatch(event, bytes) {
            if (!this.d3d9Executor ||
                    typeof this.d3d9Executor.submit !== "function") {
                console.error("[d3d9-webgpu] executor unavailable");
                return;
            }
            const payloadBytes = u32(bytes, 4);
            if (payloadBytes !== bytes.byteLength - 8) {
                console.error("[d3d9-webgpu] malformed D9WG envelope");
                return;
            }
            this.warnOnSharedD3DCanvasConflict("d3d9");
            const descriptorBase = event.descAddr >>> 0;
            const generation = this.memoryGeneration;
            const writeGuestMemory = (dmaOffset, data) => {
                if (this.destroyed || this.suspended || this.restoringState ||
                    generation !== this.memoryGeneration) return;
                const source = asBytes(data);
                if (typeof event.writeGuestMemory === "function")
                    return event.writeGuestMemory(dmaOffset, source);
                const offset = dmaOffset >>> 0;
                const end = offset + source.byteLength;
                if (end < offset || end > 16 * 1024 * 1024)
                    throw new RangeError("D9WG response write is outside DMA memory");
                if (!this.emulator ||
                        typeof this.emulator.write_memory !== "function")
                    throw new Error("v86 physical-memory writer is unavailable");
                this.emulator.write_memory(source,
                    (descriptorBase + offset) >>> 0);
            };
            return this.d3d9Executor.submit(bytes.subarray(8), {
                ...(event.replay ? { replay: true } : {}),
                pciFrameId: event.frameId >>> 0,
                submitCount: event.submitCount >>> 0,
                descriptorCommandCount: event.commandCount >>> 0,
                descriptorBase,
                writeGuestMemory,
            });
        }

        recordGraphicsBatch(event, bytes) {
            this.preparedCheckpoint = null;
            this.graphicsJournal.append(event, bytes);
            this.graphicsJournalBytes += JOURNAL_ENTRY_BYTES + bytes.byteLength;
            if (!event.barrier && !this.isEnvelope(bytes, CTRL_D3D8_BATCH) &&
                    !this.isEnvelope(bytes, CTRL_D3D9_BATCH))
                this.glJournalBytes += bytes.byteLength;
        }

        async resetJournal() {
            await this.graphicsJournal.destroy();
            this.graphicsJournal = new GraphicsJournal({ budget: this.journalBudget });
            this.graphicsJournalBytes = 0;
            this.preparedCheckpoint = null;
        }

        serializeCheckpoint() {
            if (this.restoringState || this.replayingState)
                throw new Error("graphics state is being restored");
            if (!this.preparedCheckpoint)
                throw new Error("Graphics checkpoint is not prepared; await emulator.save_state()");
            return this.preparedCheckpoint;
        }

        releaseCheckpoint() { this.preparedCheckpoint = null; }

        parseCheckpoint(checkpoint) {
            const bytes = asBytes(checkpoint);
            if (bytes.byteLength < CHECKPOINT_HEADER_BYTES)
                throw new Error("graphics checkpoint is truncated");
            const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
            const version = view.getUint16(4, true);
            if (version === 3) {
                const parsed = GraphicsJournal.parse(bytes);
                if (parsed.legacy.length && (parsed.legacy.length < CHECKPOINT_HEADER_BYTES ||
                        new DataView(parsed.legacy.buffer).getUint16(4, true) !== 1))
                    throw new Error("graphics checkpoint legacy data must be version 1");
                if (parsed.legacy.length) this.parseCheckpoint(parsed.legacy);
                return parsed;
            }
            if (view.getUint32(0, true) !== CHECKPOINT_MAGIC ||
                    (version !== 1 && version !== CHECKPOINT_VERSION))
                throw new Error("graphics checkpoint version is unsupported");
            const header = view.getUint16(6, true);
            const total = view.getUint32(8, true);
            if (header !== CHECKPOINT_HEADER_BYTES || total !== bytes.byteLength)
                throw new Error("graphics checkpoint lengths are invalid");
            if (version === 1) {
                const glBytes = view.getUint32(12, true);
                const d3d8Bytes = view.getUint32(16, true);
                if (header + glBytes + d3d8Bytes !== total)
                    throw new Error("graphics checkpoint lengths are invalid");
                return { version, gl: bytes.slice(header, header + glBytes),
                    d3d8: bytes.slice(header + glBytes), records: [] };
            }
            const legacyBytes = view.getUint32(12, true);
            if (legacyBytes > total - header)
                throw new Error("graphics checkpoint legacy data is truncated");
            const legacy = bytes.slice(header, header + legacyBytes);
            if (legacyBytes && (legacyBytes < CHECKPOINT_HEADER_BYTES ||
                    new DataView(legacy.buffer).getUint16(4, true) !== 1))
                throw new Error("graphics checkpoint legacy data must be version 1");
            if (legacyBytes) this.parseCheckpoint(legacy);
            const count = view.getUint32(16, true);
            const records = [];
            let offset = header + legacyBytes;
            for (let i = 0; i < count; ++i) {
                if (offset + JOURNAL_ENTRY_BYTES > total)
                    throw new Error("graphics checkpoint batch header is truncated");
                const size = view.getUint32(offset, true);
                const kind = view.getUint32(offset + 24, true);
                if (size > total - offset - JOURNAL_ENTRY_BYTES || kind > 1 ||
                        (kind === 1 && size) || view.getUint32(offset + 28, true))
                    throw new Error("graphics checkpoint batch is invalid");
                records.push({ flags: view.getUint32(offset + 4, true),
                    frameId: view.getUint32(offset + 8, true),
                    submitCount: view.getUint32(offset + 12, true),
                    commandCount: view.getUint32(offset + 16, true),
                    responseBase: view.getUint32(offset + 20, true), barrier: kind === 1,
                    bytes: bytes.slice(offset + JOURNAL_ENTRY_BYTES,
                        offset + JOURNAL_ENTRY_BYTES + size) });
                offset += JOURNAL_ENTRY_BYTES + size;
            }
            if (offset !== total) throw new Error("graphics checkpoint has trailing data");
            return { version, legacy, records };
        }

        async waitForIdle(flush, allowFailure = false) {
            const executors = [this.glExecutor, this.d3d8Executor, this.d3d9Executor].filter(Boolean);
            for (const executor of executors) {
                try {
                    if (executor.readyPromise) await executor.readyPromise;
                    if (executor.idle) await executor.idle();
                    else if (executor.work) await executor.work;
                } catch (error) {
                    if (!allowFailure) throw error;
                }
            }
            if (flush) {
                if (this.glExecutor && this.glExecutor.flushFrame) this.glExecutor.flushFrame();
                if (this.d3d8Executor && this.d3d8Executor.finishFrame) this.d3d8Executor.finishFrame(false);
                if (this.d3d9Executor && this.d3d9Executor.flushForCheckpoint)
                    this.d3d9Executor.flushForCheckpoint();
            }
            for (const executor of executors) {
                try {
                    if (executor.checkpointIdle) await executor.checkpointIdle();
                    else if (executor.idle) await executor.idle();
                    const queue = executor.device && executor.device.queue;
                    if (queue && queue.onSubmittedWorkDone) await queue.onSubmittedWorkDone();
                    if (executor.failed) throw executor.failed;
                } catch (error) {
                    // Loading a good checkpoint must recover an executor that
                    // failed while processing the timeline being discarded.
                    if (!allowFailure) throw error;
                }
            }
        }

        getPCIDevice() {
            const runtime = this.emulator && this.emulator.v86;
            const cpu = runtime && runtime.cpu;
            return cpu && cpu.devices && cpu.devices.v86gl_pci || null;
        }

        attachPCIStateHooks() {
            if (this.options.managedState) return true;
            const device = this.getPCIDevice();
            if (!device) return false;
            if (this.pciStateDevice === device) return true;
            if (device.__v86glStateBridge && device.__v86glStateBridge !== this)
                return false;
            if (typeof device.get_state !== "function" ||
                    typeof device.set_state !== "function") return false;
            const bridge = this;
            const originalGetState = device.get_state;
            const originalSetState = device.set_state;
            this.originalGetState = originalGetState;
            this.originalSetState = originalSetState;
            device.get_state = function() {
                const state = originalGetState.call(this);
                state[PCI_STATE_GRAPHICS_INDEX] = bridge.serializeCheckpoint();
                return state;
            };
            device.set_state = function(state) {
                originalSetState.call(this, state);
                bridge.onPCIStateRestored(state &&
                    state[PCI_STATE_GRAPHICS_INDEX]);
            };
            device.__v86glStateBridge = this;
            this.pciStateDevice = device;
            return true;
        }

        async prepareSaveState() {
            if (!this.attachPCIStateHooks())
                throw new Error("v86gl PCI device is not ready for save state");
            await this.waitForIdle(true);
            // Flushes can split an unfinished frame/query. Replay that boundary
            // too, including when this checkpoint later becomes a parent save.
            this.recordGraphicsBatch({ barrier: true }, new Uint8Array(0));
            this.preparedCheckpoint = await this.graphicsJournal.snapshot(this.legacyCheckpoint);
            return { entries: this.graphicsJournal.count, bytes: this.preparedCheckpoint.byteLength };
        }

        beginStateRestore() {
            if (!this.attachPCIStateHooks())
                throw new Error("v86gl PCI device is not ready for state restore");
            this.restorePrepared = true;
            this.restoreSeen = false;
            this.restoreHadCheckpoint = false;
            this.restoringState = true;
            this.pendingRestore = Promise.resolve();
            this.positionCanvas();
        }

        onPCIStateRestored(checkpoint) {
            ++this.memoryGeneration;
            this.restoreSeen = true;
            this.restoreHadCheckpoint = !!(checkpoint && checkpoint.byteLength);
            this.restoringState = true;
            this.pendingRestore = Promise.resolve().then(() =>
                this.restoreCheckpoint(checkpoint));
            this.pendingRestore.catch(error =>
                console.error("[v86gl] graphics state restore failed", error));
        }

        async restoreCheckpoint(checkpoint) {
            // Validate before discarding live resources. Version 1 can restore
            // only the GL/D3D8 data that old writers actually saved.
            const parsed = checkpoint && checkpoint.byteLength ? this.parseCheckpoint(checkpoint) :
                { version: 2, legacy: new Uint8Array(0), records: [] };
            await this.waitForIdle(false, true);
            this.replayingState = true;
            this.positionCanvas();
            let complete = false;
            try {
                if (this.resetForStateRestore) await this.resetForStateRestore();
                else {
                    if (this.glExecutor) this.glExecutor.resetForReplay();
                    if (this.d3d8Executor && this.d3d8Executor.restoreState)
                        await this.d3d8Executor.restoreState(new Uint8Array(0));
                    if (this.d3d9Executor && this.d3d9Executor.resetForReplay)
                        await this.d3d9Executor.resetForReplay();
                }
                await this.resetJournal();
                this.glJournal = [];
                this.glJournalBytes = 0;
                this.glJournalOverflow = false;
                this.legacyCheckpoint = parsed.version === 1 ? ownedBytes(checkpoint) : parsed.legacy;
                const legacy = parsed.version === 1 ? parsed :
                    (parsed.legacy.byteLength ? this.parseCheckpoint(parsed.legacy) : null);
                if (legacy) {
                    if (legacy.gl.byteLength) {
                        this.glExecutor.submit(legacy.gl.slice(), { replay: true });
                        this.glExecutor.onSwapBuffers();
                        this.glJournalBytes = legacy.gl.byteLength;
                    }
                    if (this.d3d8Executor && this.d3d8Executor.restoreState)
                        await this.d3d8Executor.restoreState(legacy.d3d8);
                }
                const records = parsed.version === 3 ? GraphicsJournal.records(parsed) : parsed.records;
                for await (const record of records) {
                    if (record.barrier) await this.waitForIdle(true);
                    else {
                        // Each replay owns its response buffer. Never let query
                        // responses mutate the journal or restored guest RAM.
                        const event = { ...record, replay: true, batchAddr: 0, descAddr: 0,
                            writeGuestMemory() {}, isMemoryValid: () => true };
                        await this.executePCIBatch(event, record.bytes.slice());
                    }
                    this.recordGraphicsBatch(record, record.bytes);
                    // Do not accumulate an entire decompressed history during
                    // restore. Compression and disk writes keep pace with replay.
                    await this.graphicsJournal.work;
                }
                await this.waitForIdle(false);
                complete = true;
            } finally {
                this.replayingState = false;
                this.restoringState = false;
                if (complete) {
                    this.positionCanvas();
                    this.drainPendingBatches();
                } else {
                    this.hideOverlayCanvas(true);
                    this.pendingBatches = [];
                }
            }
        }

        async finishStateRestore() {
            if (!this.restoreSeen) this.onPCIStateRestored(null);
            await this.pendingRestore;
            this.restorePrepared = false;
            return { hasGLState: this.restoreHadCheckpoint };
        }

        cancelStateRestore() {
            this.restorePrepared = false;
            this.restoreSeen = false;
            this.restoreHadCheckpoint = false;
            this.replayingState = false;
            this.restoringState = false;
            this.pendingRestore = Promise.resolve();
            this.drainPendingBatches();
        }

        drainPendingBatches() {
            const pending = this.pendingBatches.splice(0);
            for (const event of pending) this.pushPCIBatch(event);
        }

        findScreenCanvas() {
            if (!this.container ||
                    typeof this.container.getElementsByTagName !== "function")
                return null;
            const overlays = new Set([this.canvas, this.glCanvas,
                this.d3d8Canvas, this.d3d9Canvas]);
            const canvases = this.container.getElementsByTagName("canvas");
            for (let i = 0; i < canvases.length; ++i)
                if (!overlays.has(canvases[i])) return canvases[i];
            return null;
        }

        ownerCanvas(owner) {
            if (owner === "d3d8") return this.d3d8Canvas;
            if (owner === "d3d9") return this.d3d9Canvas;
            return this.glCanvas;
        }

        ownerSurface(owner) {
            if (owner === "d3d8") return this.d3d8Surface;
            if (owner === "d3d9") return this.d3d9Surface;
            return this.glSurface;
        }

        styleOverlayCanvas(canvas, left, top, width, height, visible) {
            if (!canvas || !canvas.style) return;
            if (this.destroyed || this.suspended || this.restoringState || this.replayingState ||
                    this.options.isGraphical && !this.options.isGraphical())
                visible = false;
            canvas.style.position = "absolute";
            canvas.style.left = left + "px";
            canvas.style.top = top + "px";
            styleSet(canvas, "width", width + "px", "important");
            styleSet(canvas, "height", height + "px", "important");
            styleSet(canvas, "max-width", "none", "important");
            styleSet(canvas, "max-height", "none", "important");
            styleSet(canvas, "margin", "0", "important");
            styleSet(canvas, "border", "0", "important");
            styleSet(canvas, "padding", "0", "important");
            styleSet(canvas, "transform", "none", "important");
            styleSet(canvas, "clip-path", "none", "important");
            canvas.style.pointerEvents = "none";
            styleSet(canvas, "display", visible ? "block" : "none", "important");
            canvas.style.visibility = visible ? "visible" : "hidden";
        }

        ownerRect(owner) {
            const canvas = this.ownerCanvas(owner);
            const surface = this.ownerSurface(owner);
            // DDSCL_NORMAL exposes the desktop primary. Its Present HWND is
            // merely the application asking for scanout; using that HWND's
            // client rectangle to place the canvas shrinks the entire desktop
            // texture into a splash window and makes its logo fill the window.
            // Keep the canvas coincident with the v86 desktop and let clipRect
            // reveal only the primary pixels this application actually wrote.
            const desktopPrimary = !!surface.ddDesktopPrimary;
            const w = surface.displayWidth || surface.width || canvas.width || 1;
            const h = surface.displayHeight || surface.height || canvas.height || 1;
            let left = desktopPrimary ? 0 : (surface.x || 0);
            let top = desktopPrimary ? 0 : (surface.y || 0);
            let width = w;
            let height = h;
            if (this.container && this.screenCanvas && this.screenCanvas.width &&
                    this.screenCanvas.height &&
                    typeof this.container.getBoundingClientRect === "function" &&
                    typeof this.screenCanvas.getBoundingClientRect === "function") {
                const containerRect = this.container.getBoundingClientRect();
                const screenRect = this.screenCanvas.getBoundingClientRect();
                // Rects are viewport pixels, CSS absolute offsets use the
                // container's untransformed padding box. Include scrolling
                // and borders, and undo an ancestor's axis-aligned scale.
                const zoomX = this.container.offsetWidth ? containerRect.width / this.container.offsetWidth : 1;
                const zoomY = this.container.offsetHeight ? containerRect.height / this.container.offsetHeight : 1;
                const scaleX = screenRect.width / (zoomX || 1) / this.screenCanvas.width;
                const scaleY = screenRect.height / (zoomY || 1) / this.screenCanvas.height;
                left = (screenRect.left - containerRect.left) / (zoomX || 1) -
                    (this.container.clientLeft || 0) + (this.container.scrollLeft || 0) + left * scaleX;
                top = (screenRect.top - containerRect.top) / (zoomY || 1) -
                    (this.container.clientTop || 0) + (this.container.scrollTop || 0) + top * scaleY;
                width *= scaleX;
                height *= scaleY;
            }
            return { left, top, width, height };
        }

        positionOwner(owner, visible, targetCanvas) {
            const canvas = targetCanvas || this.ownerCanvas(owner);
            if (!canvas) return;
            const surface = this.ownerSurface(owner);
            const rect = this.ownerRect(owner);
            this.styleOverlayCanvas(canvas, rect.left, rect.top,
                rect.width, rect.height,
                visible === undefined ? this.activeOwner === owner : visible);
            let clip = surface && surface.clipRect;
            // Clip windowed rendering to the VGA desktop, preserving any
            // DirectDraw primary clip region as well.
            if (this.screenCanvas && this.screenCanvas.width && this.screenCanvas.height) {
                const w = surface.displayWidth || surface.width || canvas.width || 1;
                const h = surface.displayHeight || surface.height || canvas.height || 1;
                const x = surface.ddDesktopPrimary ? 0 : surface.x || 0;
                const y = surface.ddDesktopPrimary ? 0 : surface.y || 0;
                const source = clip && clip.baseWidth > 0 && clip.baseHeight > 0 ? clip :
                    { left: 0, top: 0, right: w, bottom: h, baseWidth: w, baseHeight: h };
                clip = {
                    baseWidth: w, baseHeight: h,
                    left: Math.min(w, Math.max(0, -x, source.left / source.baseWidth * w)),
                    top: Math.min(h, Math.max(0, -y, source.top / source.baseHeight * h)),
                    right: Math.max(0, Math.min(w, this.screenCanvas.width - x, source.right / source.baseWidth * w)),
                    bottom: Math.max(0, Math.min(h, this.screenCanvas.height - y, source.bottom / source.baseHeight * h)),
                };
            }
            if (clip && clip.baseWidth > 0 && clip.baseHeight > 0) {
                const top = Math.max(0, clip.top) / clip.baseHeight * 100;
                const right = Math.max(0, clip.baseWidth - clip.right) /
                    clip.baseWidth * 100;
                const bottom = Math.max(0, clip.baseHeight - clip.bottom) /
                    clip.baseHeight * 100;
                const left = Math.max(0, clip.left) /
                    clip.baseWidth * 100;
                styleSet(canvas, "clip-path", "inset(" + top + "% " +
                    right + "% " + bottom + "% " + left + "%)",
                    "important");
            }
        }

        showOwner(owner) {
            if (this.destroyed || this.suspended) return;
            this.activeOwner = owner;
            const active = this.ownerCanvas(owner);
            for (const canvas of new Set([this.glCanvas, this.d3d8Canvas,
                    this.d3d9Canvas])) {
                if (canvas && canvas !== active)
                    this.styleOverlayCanvas(canvas, 0, 0, 0, 0, false);
            }
            this.positionOwner(owner, true);
        }

        positionCanvas() {
            if (this.activeOwner) this.positionOwner(this.activeOwner, true);
            for (const surface of this.d3d9SwapChainSurfaces.values())
                this.positionD3D9SwapChainCanvas(surface);
        }

        positionGLCanvas() { this.positionOwner("gl"); }
        positionD3D8Canvas() { this.positionOwner("d3d8"); }
        positionD3D9Canvas() { this.positionOwner("d3d9"); }

        hideOverlayCanvas(includeSwapChains) {
            for (const canvas of new Set([this.glCanvas, this.d3d8Canvas,
                    this.d3d9Canvas]))
                if (canvas) this.styleOverlayCanvas(canvas, 0, 0, 0, 0, false);
            this.activeOwner = null;
            if (includeSwapChains) {
                for (const canvas of this.d3d9SwapChainCanvases.values())
                    this.styleOverlayCanvas(canvas, 0, 0, 0, 0, false);
            }
        }

        showOverlayCanvas() { this.showOwner("gl"); }
        hideGLCanvas() {
            if (this.activeOwner === "gl") this.hideOverlayCanvas();
        }
        showD3D8Canvas() { this.showOwner("d3d8"); }
        hideD3D8Canvas() {
            if (this.activeOwner === "d3d8") this.hideOverlayCanvas();
        }
        showD3D9Canvas() { this.showOwner("d3d9"); }
        hideD3D9Canvas() {
            if (this.activeOwner === "d3d9") this.hideOverlayCanvas();
        }

        positionD3D9SwapChainCanvas(surface) {
            this.d3d9SwapChainSurfaces.set(surface.swapChain, surface);
            const canvas = this.d3d9SwapChainCanvases.get(surface.swapChain);
            if (!canvas) return;
            const saved = this.d3d9Surface;
            this.d3d9Surface = { ...saved, ...surface };
            this.positionOwner("d3d9", surface.visible !== false, canvas);
            this.d3d9Surface = saved;
        }

        removeD3D9SwapChainCanvas(handle) {
            const canvas = this.d3d9SwapChainCanvases.get(handle);
            if (!canvas) return;
            this.d3d9SwapChainCanvases.delete(handle);
            this.d3d9SwapChainSurfaces.delete(handle);
            if (canvas.className === "v86gl-d3d9-swapchain-overlay" &&
                    canvas.parentElement)
                canvas.parentElement.removeChild(canvas);
            else this.styleOverlayCanvas(canvas, 0, 0, 0, 0, false);
        }

        warnOnSharedD3DCanvasConflict(active) {
            if (active === "d3d8") this.d3d8BatchStreamSeen = true;
            if (active === "d3d9") this.d3d9BatchStreamSeen = true;
            if (!this.d3d8BatchStreamSeen || !this.d3d9BatchStreamSeen ||
                    this.d3d8Canvas !== this.d3d9Canvas ||
                    this.sharedD3DCanvasConflictReported) return;
            this.sharedD3DCanvasConflictReported = true;
            console.error("[v86gl] D3D8 and D3D9 streams cannot concurrently " +
                "own the same WebGPU canvas", { drivingThisBatch: active });
        }
    }

    global.V86GL_BRIDGE_VERSION = V86GL_BRIDGE_VERSION;
    global.installV86GLNetworkBridge = function(emulator, canvas, options) {
        return new V86GLNetworkBridge(emulator, canvas, options);
    };
})(typeof window !== "undefined" ? window : globalThis);
