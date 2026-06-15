# Simple v86 OpenGL wrapper

This is a deliberately tiny fake `opengl32.dll` for experiments in v86.

It does **not** implement real OpenGL. It captures a very small subset of WGL/OpenGL calls and sends compact `VGL1` command packets to COM1. A browser-side JavaScript bridge receives the bytes from v86 serial0 and renders a basic immediate-mode triangle stream with WebGL.

## What it supports

Guest DLL exports:

- `wglCreateContext`
- `wglDeleteContext`
- `wglMakeCurrent`
- `wglGetCurrentContext`
- `wglGetCurrentDC`
- `wglGetProcAddress`
- `wglShareLists`
- `wglSwapLayerBuffers`
- `wglSwapBuffers`
- `glGetString`
- `glGetError`
- `glViewport`
- `glClearColor`
- `glClear`
- `glBegin`
- `glEnd`
- `glColor3f`
- `glColor4f`
- `glVertex2f`
- `glVertex3f`
- `glFlush`
- `glFinish`

It is enough for a toy OpenGL triangle demo. It is **not** enough for WineD3D or real games yet.

## Build the DLL

From Linux/macOS with mingw-w64:

```bash
cd src/glbridge/winproxy
i686-w64-mingw32-gcc -shared -Os -s \
  -o opengl32.dll opengl32_proxy.c opengl32.def \
  -Wl,--kill-at
```

Build the test program:

```bash
cd src/glbridge/sample
i686-w64-mingw32-gcc -mwindows -Os -s \
  -o gl_triangle_test.exe gl_triangle_test.c \
  -lopengl32 -lgdi32 -luser32
```

Copy both files into the same folder in the Windows XP guest:

```text
opengl32.dll
gl_triangle_test.exe
```

Run `gl_triangle_test.exe`.

The demo calls the fake WGL/OpenGL subset directly and presents with
`wglSwapLayerBuffers` plus `glFlush`, so it does not depend on intercepting
`gdi32.dll`'s real `SwapBuffers`.

## v86 browser side

Include `v86gl_serial_bridge.js` in your v86 page. Add an overlay canvas above the v86 screen canvas:

```html
<canvas id="v86gl_canvas"
        style="position:absolute;left:0;top:0;display:none;pointer-events:none"></canvas>
```

After creating the v86 emulator:

```js
const v86gl = installV86GLSerialBridge(
    emulator,
    document.getElementById("v86gl_canvas")
);
```

The bridge listens to:

```js
emulator.add_listener("serial0-output-byte", ...)
```

and renders the received command stream into the WebGL canvas.

For a host-only smoke test, open:

```text
src/glbridge/sample/host_triangle_demo.html
```

That page uses a tiny emulator stub and feeds the same `VGL1` serial packets to
`v86gl_serial_bridge.js`, without booting Windows XP.

## Important limitations

- COM1 is only for proof of concept. It is too slow for real games.
- There is no matrix stack, texture support, depth testing, clipping, lighting, or WineD3D compatibility.
- `SwapBuffers` is exported by `gdi32.dll`, not `opengl32.dll`; normal apps that import `SwapBuffers` from `gdi32.dll` are not intercepted by this DLL. This toy bridge presents on `glFlush`, `glFinish`, `wglSwapLayerBuffers`, and the nonstandard helper export `wglSwapBuffers`.
- For real performance, replace COM1 with a v86 PCI/MMIO shared command ring and batch commands per frame.
