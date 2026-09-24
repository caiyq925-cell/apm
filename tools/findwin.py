import ctypes, ctypes.wintypes as w, sys

u = ctypes.windll.user32
u.GetWindowTextLengthW.restype = ctypes.c_int
EnumProc = ctypes.WINFUNCTYPE(ctypes.c_bool, w.HWND, w.LPARAM)
want = int(sys.argv[1])
found = []


def cb(h, l):
    pid = w.DWORD()
    u.GetWindowThreadProcessId(h, ctypes.byref(pid))
    if pid.value == want:
        n = u.GetWindowTextLengthW(h)
        buf = ctypes.create_unicode_buffer(n + 1)
        u.GetWindowTextW(h, buf, n + 1)
        r = w.RECT()
        u.GetWindowRect(h, ctypes.byref(r))
        found.append((h, u.IsWindowVisible(h), buf.value, r.right - r.left, r.bottom - r.top))
    return True


u.EnumWindows(EnumProc(cb), 0)
for h, vis, title, ww, hh in found:
    print(f"hwnd={h} vis={vis} title={title!r} {ww}x{hh}")
big = [f for f in found if f[1] and f[3] > 400 and f[4] > 300]
if big:
    print("MAIN", big[0][0])
