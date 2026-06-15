// Tiny Win32 OpenGL test program for simple_v86_opengl_wrapper.
// Build:
//   i686-w64-mingw32-gcc -mwindows -Os -s -o gl_triangle_test.exe gl_triangle_test.c -lopengl32 -lgdi32 -luser32

#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <string.h>

#ifndef APIENTRY
#define APIENTRY __stdcall
#endif

typedef unsigned int GLenum;
typedef unsigned int GLbitfield;
typedef int GLint;
typedef int GLsizei;
typedef float GLfloat;

#define GL_COLOR_BUFFER_BIT 0x00004000
#define GL_TRIANGLES        0x0004

HGLRC APIENTRY wglCreateContext(HDC);
BOOL  APIENTRY wglDeleteContext(HGLRC);
BOOL  APIENTRY wglMakeCurrent(HDC, HGLRC);
BOOL  APIENTRY wglSwapLayerBuffers(HDC, UINT);
void APIENTRY glViewport(GLint x, GLint y, GLsizei width, GLsizei height);
void APIENTRY glClearColor(GLfloat r, GLfloat g, GLfloat b, GLfloat a);
void APIENTRY glClear(GLbitfield mask);
void APIENTRY glBegin(GLenum mode);
void APIENTRY glEnd(void);
void APIENTRY glColor3f(GLfloat r, GLfloat g, GLfloat b);
void APIENTRY glVertex3f(GLfloat x, GLfloat y, GLfloat z);
void APIENTRY glFlush(void);

#define V86GL_TIMER_ID 1
#define WGL_SWAP_MAIN_PLANE 0x00000001

static HGLRC g_rc = NULL;
static HDC   g_dc = NULL;

static void draw_triangle(HWND hwnd) {
    RECT rc;
    GetClientRect(hwnd, &rc);
    glViewport(0, 0, rc.right - rc.left, rc.bottom - rc.top);
    glClearColor(0.05f, 0.08f, 0.12f, 1.0f);
    glClear(GL_COLOR_BUFFER_BIT);
    glBegin(GL_TRIANGLES);
    glColor3f(1.0f, 0.1f, 0.1f);
    glVertex3f(-0.75f, -0.65f, 0.0f);
    glColor3f(0.1f, 1.0f, 0.1f);
    glVertex3f(0.75f, -0.65f, 0.0f);
    glColor3f(0.1f, 0.3f, 1.0f);
    glVertex3f(0.0f, 0.75f, 0.0f);
    glEnd();
    wglSwapLayerBuffers(g_dc, WGL_SWAP_MAIN_PLANE);
    glFlush();
}

static LRESULT CALLBACK wndproc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp) {
    switch (msg) {
    case WM_CREATE: {
        PIXELFORMATDESCRIPTOR pfd;
        memset(&pfd, 0, sizeof(pfd));
        pfd.nSize = sizeof(pfd);
        pfd.nVersion = 1;
        pfd.dwFlags = PFD_DRAW_TO_WINDOW | PFD_SUPPORT_OPENGL | PFD_DOUBLEBUFFER;
        pfd.iPixelType = PFD_TYPE_RGBA;
        pfd.cColorBits = 24;
        pfd.cDepthBits = 16;
        pfd.iLayerType = PFD_MAIN_PLANE;
        g_dc = GetDC(hwnd);
        int pf = ChoosePixelFormat(g_dc, &pfd);
        if (pf > 0) {
            SetPixelFormat(g_dc, pf, &pfd);
        }
        g_rc = wglCreateContext(g_dc);
        wglMakeCurrent(g_dc, g_rc);
        SetTimer(hwnd, V86GL_TIMER_ID, 33, NULL);
        return 0;
    }
    case WM_ERASEBKGND:
        return 1;
    case WM_PAINT: {
        PAINTSTRUCT ps;
        BeginPaint(hwnd, &ps);
        draw_triangle(hwnd);
        EndPaint(hwnd, &ps);
        return 0;
    }
    case WM_TIMER:
        if (wp == V86GL_TIMER_ID) {
            draw_triangle(hwnd);
            return 0;
        }
        break;
    case WM_SIZE:
        InvalidateRect(hwnd, NULL, TRUE);
        return 0;
    case WM_DESTROY:
        KillTimer(hwnd, V86GL_TIMER_ID);
        if (g_rc) {
            wglMakeCurrent(NULL, NULL);
            wglDeleteContext(g_rc);
        }
        if (g_dc) {
            ReleaseDC(hwnd, g_dc);
        }
        PostQuitMessage(0);
        return 0;
    }
    return DefWindowProc(hwnd, msg, wp, lp);
}

int WINAPI WinMain(HINSTANCE hinst, HINSTANCE prev, LPSTR cmd, int show) {
    (void)prev;
    (void)cmd;
    WNDCLASSA wc;
    memset(&wc, 0, sizeof(wc));
    wc.lpfnWndProc = wndproc;
    wc.hInstance = hinst;
    wc.lpszClassName = "V86GLTriangle";
    wc.hCursor = LoadCursor(NULL, IDC_ARROW);
    RegisterClassA(&wc);
    HWND hwnd = CreateWindowA("V86GLTriangle", "v86 fake OpenGL triangle",
        WS_OVERLAPPEDWINDOW | WS_VISIBLE, CW_USEDEFAULT, CW_USEDEFAULT,
        640, 480, NULL, NULL, hinst, NULL);
    ShowWindow(hwnd, show);
    UpdateWindow(hwnd);
    MSG msg;
    while (GetMessage(&msg, NULL, 0, 0) > 0) {
        TranslateMessage(&msg);
        DispatchMessage(&msg);
    }
    return 0;
}
