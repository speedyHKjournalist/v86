// Minimal fake opengl32.dll for v86 experiments.
// It serializes a tiny OpenGL subset to COM1 using VGL1 packets.
// Build:
//   i686-w64-mingw32-gcc -shared -Os -s -o opengl32.dll opengl32_proxy.c opengl32.def -Wl,--kill-at

#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdint.h>
#include <string.h>

#ifndef APIENTRY
#define APIENTRY __stdcall
#endif

typedef unsigned int GLenum;
typedef unsigned int GLbitfield;
typedef unsigned char GLubyte;
typedef int GLint;
typedef int GLsizei;
typedef float GLfloat;

#define GL_VENDOR             0x1F00
#define GL_RENDERER           0x1F01
#define GL_VERSION            0x1F02
#define GL_EXTENSIONS         0x1F03
#define GL_COLOR_BUFFER_BIT   0x00004000
#define GL_DEPTH_BUFFER_BIT   0x00000100
#define GL_POINTS             0x0000
#define GL_LINES              0x0001
#define GL_TRIANGLES          0x0004
#define GL_TRIANGLE_STRIP     0x0005
#define GL_TRIANGLE_FAN       0x0006
#define GL_QUADS              0x0007

#define VGL_MAGIC 0x314C4756u  // 'VGL1' little-endian

enum {
    OP_MAKE_CURRENT = 1,
    OP_VIEWPORT     = 2,
    OP_CLEAR_COLOR  = 3,
    OP_CLEAR        = 4,
    OP_BEGIN        = 5,
    OP_END          = 6,
    OP_COLOR4F      = 7,
    OP_VERTEX3F     = 8,
    OP_PRESENT      = 9,
};

#pragma pack(push, 1)
typedef struct {
    uint32_t magic;
    uint16_t op;
    uint16_t size;
    uint32_t seq;
} VGLHeader;
#pragma pack(pop)

static HANDLE g_com = INVALID_HANDLE_VALUE;
static HDC    g_current_dc = NULL;
static HGLRC  g_current_ctx = NULL;
static uint32_t g_seq = 1;
static GLenum g_error = 0;

static int open_com1(void) {
    if (g_com != INVALID_HANDLE_VALUE) {
        return 1;
    }

    g_com = CreateFileA(
        "\\\\.\\COM1",
        GENERIC_READ | GENERIC_WRITE,
        0,
        NULL,
        OPEN_EXISTING,
        FILE_ATTRIBUTE_NORMAL,
        NULL
    );

    if (g_com == INVALID_HANDLE_VALUE) {
        return 0;
    }

    DCB dcb;
    memset(&dcb, 0, sizeof(dcb));
    dcb.DCBlength = sizeof(dcb);

    if (GetCommState(g_com, &dcb)) {
        dcb.fBinary = TRUE;
        dcb.fOutxCtsFlow = FALSE;
        dcb.fOutxDsrFlow = FALSE;
        dcb.fDsrSensitivity = FALSE;
        dcb.fOutX = FALSE;
        dcb.fInX = FALSE;
        dcb.fDtrControl = DTR_CONTROL_ENABLE;
        dcb.fRtsControl = RTS_CONTROL_ENABLE;
        dcb.BaudRate = CBR_115200;
        dcb.ByteSize = 8;
        dcb.Parity = NOPARITY;
        dcb.StopBits = ONESTOPBIT;
        SetCommState(g_com, &dcb);
    }

    COMMTIMEOUTS timeouts;
    memset(&timeouts, 0, sizeof(timeouts));
    timeouts.WriteTotalTimeoutMultiplier = 1;
    timeouts.WriteTotalTimeoutConstant = 10;
    SetCommTimeouts(g_com, &timeouts);

    return 1;
}

static void write_all(const void* data, DWORD size) {
    const uint8_t* p = (const uint8_t*)data;

    while (size) {
        DWORD written = 0;

        if (!WriteFile(g_com, p, size, &written, NULL) || !written) {
            break;
        }

        p += written;
        size -= written;
    }
}

static void emit_packet(uint16_t op, const void* payload, uint16_t size) {
    if (!open_com1()) {
        return;
    }

    VGLHeader h;
    h.magic = VGL_MAGIC;
    h.op = op;
    h.size = size;
    h.seq = g_seq++;

    write_all(&h, sizeof(h));

    if (payload && size) {
        write_all(payload, size);
    }
}

static void emit_present(void) {
    emit_packet(OP_PRESENT, NULL, 0);
}

BOOL WINAPI DllMain(HINSTANCE hinst, DWORD reason, LPVOID reserved) {
    (void)hinst;
    (void)reserved;

    if (reason == DLL_PROCESS_DETACH) {
        if (g_com != INVALID_HANDLE_VALUE) {
            CloseHandle(g_com);
            g_com = INVALID_HANDLE_VALUE;
        }
    }

    return TRUE;
}

__declspec(dllexport)
HGLRC APIENTRY wglCreateContext(HDC hdc) {
    (void)hdc;
    return (HGLRC)0x1001;
}

__declspec(dllexport)
BOOL APIENTRY wglDeleteContext(HGLRC ctx) {
    (void)ctx;
    return TRUE;
}

__declspec(dllexport)
BOOL APIENTRY wglMakeCurrent(HDC hdc, HGLRC ctx) {
    g_current_dc = hdc;
    g_current_ctx = ctx;

    if (!hdc || !ctx) {
        return TRUE;
    }

    HWND hwnd = hdc ? WindowFromDC(hdc) : NULL;
    RECT rc;
    memset(&rc, 0, sizeof(rc));

    if (hwnd) {
        GetClientRect(hwnd, &rc);
    }

    struct {
        uint32_t hwnd;
        uint32_t width;
        uint32_t height;
    } payload;

    payload.hwnd = (uint32_t)(uintptr_t)hwnd;
    payload.width = (uint32_t)(rc.right - rc.left);
    payload.height = (uint32_t)(rc.bottom - rc.top);

    if (!payload.width) {
        payload.width = 640;
    }

    if (!payload.height) {
        payload.height = 480;
    }

    emit_packet(OP_MAKE_CURRENT, &payload, sizeof(payload));
    return TRUE;
}

__declspec(dllexport)
HGLRC APIENTRY wglGetCurrentContext(void) {
    return g_current_ctx;
}

__declspec(dllexport)
HDC APIENTRY wglGetCurrentDC(void) {
    return g_current_dc;
}

__declspec(dllexport)
BOOL APIENTRY wglShareLists(HGLRC a, HGLRC b) {
    (void)a;
    (void)b;
    return TRUE;
}

__declspec(dllexport)
BOOL APIENTRY wglSwapLayerBuffers(HDC hdc, UINT planes) {
    (void)hdc;
    (void)planes;
    emit_present();
    return TRUE;
}

__declspec(dllexport)
BOOL APIENTRY wglSwapBuffers(HDC hdc) {
    (void)hdc;
    emit_present();
    return TRUE;
}

__declspec(dllexport)
PROC APIENTRY wglGetProcAddress(LPCSTR name) {
    (void)name;
    return NULL;
}

__declspec(dllexport)
const GLubyte* APIENTRY glGetString(GLenum name) {
    switch (name) {
    case GL_VENDOR:     return (const GLubyte*)"v86";
    case GL_RENDERER:   return (const GLubyte*)"v86 fake OpenGL over serial";
    case GL_VERSION:    return (const GLubyte*)"1.1";
    case GL_EXTENSIONS: return (const GLubyte*)"";
    default:            return (const GLubyte*)"";
    }
}

__declspec(dllexport)
GLenum APIENTRY glGetError(void) {
    GLenum e = g_error;
    g_error = 0;
    return e;
}

__declspec(dllexport)
void APIENTRY glViewport(GLint x, GLint y, GLsizei width, GLsizei height) {
    struct { int32_t x, y, width, height; } payload;
    payload.x = x;
    payload.y = y;
    payload.width = width;
    payload.height = height;
    emit_packet(OP_VIEWPORT, &payload, sizeof(payload));
}

__declspec(dllexport)
void APIENTRY glClearColor(GLfloat r, GLfloat g, GLfloat b, GLfloat a) {
    struct { float r, g, b, a; } payload;
    payload.r = r;
    payload.g = g;
    payload.b = b;
    payload.a = a;
    emit_packet(OP_CLEAR_COLOR, &payload, sizeof(payload));
}

__declspec(dllexport)
void APIENTRY glClear(GLbitfield mask) {
    uint32_t payload = (uint32_t)mask;
    emit_packet(OP_CLEAR, &payload, sizeof(payload));
}

__declspec(dllexport)
void APIENTRY glBegin(GLenum mode) {
    uint32_t payload = (uint32_t)mode;
    emit_packet(OP_BEGIN, &payload, sizeof(payload));
}

__declspec(dllexport)
void APIENTRY glEnd(void) {
    emit_packet(OP_END, NULL, 0);
}

__declspec(dllexport)
void APIENTRY glColor3f(GLfloat r, GLfloat g, GLfloat b) {
    struct { float r, g, b, a; } payload;
    payload.r = r;
    payload.g = g;
    payload.b = b;
    payload.a = 1.0f;
    emit_packet(OP_COLOR4F, &payload, sizeof(payload));
}

__declspec(dllexport)
void APIENTRY glColor4f(GLfloat r, GLfloat g, GLfloat b, GLfloat a) {
    struct { float r, g, b, a; } payload;
    payload.r = r;
    payload.g = g;
    payload.b = b;
    payload.a = a;
    emit_packet(OP_COLOR4F, &payload, sizeof(payload));
}

__declspec(dllexport)
void APIENTRY glVertex2f(GLfloat x, GLfloat y) {
    struct { float x, y, z; } payload;
    payload.x = x;
    payload.y = y;
    payload.z = 0.0f;
    emit_packet(OP_VERTEX3F, &payload, sizeof(payload));
}

__declspec(dllexport)
void APIENTRY glVertex3f(GLfloat x, GLfloat y, GLfloat z) {
    struct { float x, y, z; } payload;
    payload.x = x;
    payload.y = y;
    payload.z = z;
    emit_packet(OP_VERTEX3F, &payload, sizeof(payload));
}

__declspec(dllexport)
void APIENTRY glFlush(void) {
    emit_present();
}

__declspec(dllexport)
void APIENTRY glFinish(void) {
    emit_present();
}
