import ctypes, ctypes.wintypes as w, sys, time

u = ctypes.windll.user32
u.GetWindowTextLengthW.restype = ctypes.c_int
u.GetClassNameW.restype = ctypes.c_int
EnumProc = ctypes.WINFUNCTYPE(ctypes.c_bool, w.HWND, w.LPARAM)


def info(h):
    n = u.GetWindowTextLengthW(h)
    tb = ctypes.create_unicode_buffer(n + 1)
    u.GetWindowTextW(h, tb, n + 1)
    cb = ctypes.create_unicode_buffer(256)
    u.GetClassNameW(h, cb, 256)
    r = w.RECT()
    u.GetWindowRect(h, ctypes.byref(r))
    return (h, cb.value, tb.value, u.IsWindowVisible(h), r.right - r.left, r.bottom - r.top)


def windows_for(pid):
    out = []

    def cb(h, l):
        p = w.DWORD()
        u.GetWindowThreadProcessId(h, ctypes.byref(p))
        if p.value == pid:
            out.append(info(h))

            def cb2(c, l2):
                out.append(info(c))
                return True
            u.EnumChildWindows(h, EnumProc(cb2), 0)
        return True

    u.EnumWindows(EnumProc(cb), 0)
    return out


pid = int(sys.argv[1])
secs = int(sys.argv[2]) if len(sys.argv) > 2 else 1
prev = set()
deadline = time.time() + secs
while True:
    cur = windows_for(pid)
    for it in cur:
        if it[0] not in prev:
            print(f"NEW hwnd={it[0]} class={it[1]!r} title={it[2]!r} vis={it[3]} {it[4]}x{it[5]}")
    prev = {it[0] for it in cur}
    if time.time() >= deadline:
        break
    time.sleep(0.7)
print("--- 当前全部窗口 ---")
for it in windows_for(pid):
    print(f"hwnd={it[0]} class={it[1]!r} title={it[2]!r} vis={it[3]} {it[4]}x{it[5]}")
