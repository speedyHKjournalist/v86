// Minimal v86 serial0 -> WebGL bridge for simple_v86_opengl_wrapper.
// Usage:
//   const v86gl = installV86GLSerialBridge(emulator, document.getElementById("v86gl_canvas"));

(function(global) {
    "use strict";
    const OP_MAKE_CURRENT = 1, OP_VIEWPORT = 2, OP_CLEAR_COLOR = 3, OP_CLEAR = 4;
    const OP_BEGIN = 5, OP_END = 6, OP_COLOR4F = 7, OP_VERTEX3F = 8, OP_PRESENT = 9;
    const GL_COLOR_BUFFER_BIT = 0x00004000, GL_DEPTH_BUFFER_BIT = 0x00000100;
    const GL_TRIANGLES = 0x0004, GL_TRIANGLE_STRIP = 0x0005, GL_TRIANGLE_FAN = 0x0006, GL_QUADS = 0x0007;

    function u16(a,o){ return a[o] | (a[o+1] << 8); }
    function u32(a,o){ return (a[o] | (a[o+1]<<8) | (a[o+2]<<16) | (a[o+3]<<24)) >>> 0; }
    function i32(a,o){ return u32(a,o) | 0; }
    function f32(a,o){ const b = new Uint8Array([a[o], a[o+1], a[o+2], a[o+3]]); return new DataView(b.buffer).getFloat32(0, true); }

    function shader(gl, type, src) {
        const s = gl.createShader(type);
        gl.shaderSource(s, src);
        gl.compileShader(s);
        if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s));
        return s;
    }

    function program(gl) {
        const vs = shader(gl, gl.VERTEX_SHADER, "attribute vec3 a_pos; attribute vec4 a_color; varying vec4 v_color; void main(){ gl_Position=vec4(a_pos,1.0); v_color=a_color; }");
        const fs = shader(gl, gl.FRAGMENT_SHADER, "precision mediump float; varying vec4 v_color; void main(){ gl_FragColor=v_color; }");
        const p = gl.createProgram();
        gl.attachShader(p, vs); gl.attachShader(p, fs); gl.linkProgram(p);
        if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(p));
        return p;
    }

    class V86GLSerialBridge {
        constructor(emulator, canvas) {
            this.emulator = emulator;
            this.canvas = canvas;
            this.buf = [];
            this.gl = canvas.getContext("webgl", { alpha: true, antialias: false, depth: true, preserveDrawingBuffer: true });
            if (!this.gl) throw new Error("WebGL is not available");
            const gl = this.gl;
            this.program = program(gl);
            this.posLoc = gl.getAttribLocation(this.program, "a_pos");
            this.colorLoc = gl.getAttribLocation(this.program, "a_color");
            this.vbo = gl.createBuffer();
            this.currentColor = [1,1,1,1];
            this.primitiveMode = 0;
            this.vertices = [];
            emulator.add_listener("serial0-output-byte", byte => this.pushByte(byte));
        }
        pushByte(byte) { this.buf.push(byte & 0xFF); this.parse(); }
        parse() {
            while (this.buf.length >= 12) {
                if (this.buf[0] !== 0x56 || this.buf[1] !== 0x47 || this.buf[2] !== 0x4C || this.buf[3] !== 0x31) {
                    this.buf.shift(); continue;
                }
                const op = u16(this.buf, 4), size = u16(this.buf, 6), total = 12 + size;
                if (this.buf.length < total) return;
                const payload = this.buf.slice(12, total);
                this.buf.splice(0, total);
                this.dispatch(op, payload);
            }
        }
        dispatch(op, p) {
            const gl = this.gl;
            switch (op) {
            case OP_MAKE_CURRENT: this.resize(u32(p,4) || 640, u32(p,8) || 480); break;
            case OP_VIEWPORT: this.resize(i32(p,8), i32(p,12)); gl.viewport(i32(p,0), i32(p,4), i32(p,8), i32(p,12)); break;
            case OP_CLEAR_COLOR: gl.clearColor(f32(p,0), f32(p,4), f32(p,8), f32(p,12)); break;
            case OP_CLEAR: {
                const mask = u32(p,0); let m = 0;
                if (mask & GL_COLOR_BUFFER_BIT) m |= gl.COLOR_BUFFER_BIT;
                if (mask & GL_DEPTH_BUFFER_BIT) m |= gl.DEPTH_BUFFER_BIT;
                gl.clear(m); break;
            }
            case OP_BEGIN: this.primitiveMode = u32(p,0); this.vertices = []; break;
            case OP_COLOR4F: this.currentColor = [f32(p,0), f32(p,4), f32(p,8), f32(p,12)]; break;
            case OP_VERTEX3F: this.vertices.push(f32(p,0), f32(p,4), f32(p,8), this.currentColor[0], this.currentColor[1], this.currentColor[2], this.currentColor[3]); break;
            case OP_END: this.drawImmediate(); break;
            case OP_PRESENT: this.present(); break;
            }
        }
        resize(w,h) {
            if (w <= 0 || h <= 0) return;
            if (this.canvas.width !== w || this.canvas.height !== h) {
                this.canvas.width = w; this.canvas.height = h;
                this.canvas.style.width = w + "px"; this.canvas.style.height = h + "px";
            }
            this.canvas.style.display = "block";
        }
        drawImmediate() {
            const gl = this.gl;
            if (!this.vertices.length) return;
            let mode = gl.TRIANGLES, data = this.vertices;
            if (this.primitiveMode === GL_TRIANGLE_STRIP) mode = gl.TRIANGLE_STRIP;
            else if (this.primitiveMode === GL_TRIANGLE_FAN) mode = gl.TRIANGLE_FAN;
            else if (this.primitiveMode === GL_QUADS) {
                const out = [];
                for (let i=0; i + 28 <= data.length; i += 28) {
                    const v0=data.slice(i,i+7), v1=data.slice(i+7,i+14), v2=data.slice(i+14,i+21), v3=data.slice(i+21,i+28);
                    out.push(...v0,...v1,...v2,...v0,...v2,...v3);
                }
                data = out; mode = gl.TRIANGLES;
            }
            const f = new Float32Array(data);
            gl.useProgram(this.program);
            gl.bindBuffer(gl.ARRAY_BUFFER, this.vbo);
            gl.bufferData(gl.ARRAY_BUFFER, f, gl.STREAM_DRAW);
            gl.enableVertexAttribArray(this.posLoc);
            gl.vertexAttribPointer(this.posLoc, 3, gl.FLOAT, false, 28, 0);
            gl.enableVertexAttribArray(this.colorLoc);
            gl.vertexAttribPointer(this.colorLoc, 4, gl.FLOAT, false, 28, 12);
            gl.drawArrays(mode, 0, f.length / 7);
        }
        present() { this.gl.flush(); this.canvas.style.display = "block"; }
    }

    global.installV86GLSerialBridge = function(emulator, canvas) {
        return new V86GLSerialBridge(emulator, canvas);
    };
})(typeof window !== "undefined" ? window : globalThis);
