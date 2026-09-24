"""启动打包好的 exe，验证界面真的渲染出来，并把窗口截图存成 PNG。

用途：这是"包能不能用"的唯一可靠判据。只看"进程活着 + 窗口存在"是不够的 ——
WebView2 用户数据目录损坏时窗口照样在，但内容是全白（实测）。
"""
import ctypes
import os
import struct
import subprocess
import sys
import time
import zlib
from ctypes import wintypes

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import _cdp

u = ctypes.windll.user32
g = ctypes.windll.gdi32


class BIH(ctypes.Structure):
    _fields_ = [("biSize", wintypes.DWORD), ("biWidth", ctypes.c_long), ("biHeight", ctypes.c_long),
                ("biPlanes", wintypes.WORD), ("biBitCount", wintypes.WORD), ("biCompression", wintypes.DWORD),
                ("biSizeImage", wintypes.DWORD), ("biXPelsPerMeter", ctypes.c_long),
                ("biYPelsPerMeter", ctypes.c_long), ("biClrUsed", wintypes.DWORD), ("biClrImportant", wintypes.DWORD)]


def png_chunk(tag, data):
    return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)


def main():
    exe = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "dist", "apm-monitor.exe"))
    out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "_shot_ok.png")
    p = subprocess.Popen([exe], cwd=os.path.dirname(exe))
    print("PID =", p.pid)
    # 调试端口只在启动后几秒内可用，必须趁早查（等太久会拿不到目标，误判成白屏）
    time.sleep(4)
    print("进程存活 =", p.poll() is None)

    t = _cdp.targets()
    pages = [x for x in t if x.get("type") == "page"]
    print("targets =", [(x.get("type"), x.get("url")) for x in t])
    ok = False
    if pages:
        cdp = _cdp.CDP(pages[0]["webSocketDebuggerUrl"])
        try:
            cdp.cmd("Runtime.enable")
            href = cdp.evaluate("location.href")
            scripts = cdp.evaluate("document.scripts.length")
            sheets = cdp.evaluate("document.styleSheets.length")
            body = cdp.evaluate("document.body ? document.body.innerHTML.length : -1")
            print("  location.href   =", href)
            print("  scripts 数      =", scripts)
            print("  styleSheets 数  =", sheets)
            print("  body 长度       =", body)
            print("  标题            =", cdp.evaluate("document.title"))
            print("  设置区 #settings=", cdp.evaluate("!!document.querySelector('#settings')"))
            print("  扫码登录按钮    =", cdp.evaluate("(document.querySelector('#btn-cloud-login')||{}).textContent||'(无)'"))
            print("  开始统计按钮    =", cdp.evaluate("(document.querySelector('#btn-query-2')||{}).textContent||'(无)'"))
            ok = bool(scripts) and body and body > 0
        finally:
            cdp.close()
    print()
    print("界面渲染正常 =", ok, "（False 基本就是 WebView2 数据目录坏了，见下面说明）")

    hw = [None]

    def cb(h, l):
        pid = wintypes.DWORD()
        u.GetWindowThreadProcessId(h, ctypes.byref(pid))
        if pid.value == p.pid and u.IsWindowVisible(h):
            r = wintypes.RECT()
            u.GetWindowRect(h, ctypes.byref(r))
            if r.right - r.left > 400 and r.bottom - r.top > 300:
                hw[0] = h
                return False
        return True

    u.EnumWindows(ctypes.WINFUNCTYPE(ctypes.c_bool, wintypes.HWND, wintypes.LPARAM)(cb), 0)
    if hw[0]:
        r = wintypes.RECT()
        u.GetWindowRect(hw[0], ctypes.byref(r))
        w, h = r.right - r.left, r.bottom - r.top
        hdc = u.GetWindowDC(hw[0])
        mem = g.CreateCompatibleDC(hdc)
        bih = BIH()
        bih.biSize = ctypes.sizeof(BIH)
        bih.biWidth, bih.biHeight = w, -h
        bih.biPlanes, bih.biBitCount = 1, 32
        bits = ctypes.c_void_p()
        hbmp = g.CreateDIBSection(mem, ctypes.byref(bih), 0, ctypes.byref(bits), None, 0)
        g.SelectObject(mem, hbmp)
        u.PrintWindow(hw[0], mem, 2)
        buf = ctypes.string_at(bits, w * h * 4)
        rows = []
        for y in range(h):
            row = bytearray()
            for x in range(w):
                i = (y * w + x) * 4
                row += bytes((buf[i + 2], buf[i + 1], buf[i]))
            rows.append(bytes(row))
        raw = b"".join(b"\x00" + rr for rr in rows)
        png = (b"\x89PNG\r\n\x1a\n"
               + png_chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
               + png_chunk(b"IDAT", zlib.compress(raw, 6))
               + png_chunk(b"IEND", b""))
        open(out, "wb").write(png)
        print("截图已存", out, w, "x", h)

    # ⚠️ 必须**正常关闭**：taskkill /F 会把 WebView2 用户数据目录弄坏，下次启动就白屏。
    # 见 docs/HANDOFF-会话与登录改造.md 坑九。
    # WebView2 拆卸较慢，实测要十几秒；超时了也只用 taskkill（不带 /F，它发 WM_CLOSE）。
    if hw[0]:
        u.PostMessageW(hw[0], 0x0010, 0, 0)   # WM_CLOSE
    for _ in range(60):
        if p.poll() is not None:
            break
        time.sleep(0.5)
    if p.poll() is None:
        print("优雅关闭未生效，改用 taskkill（不带 /F）…")
        subprocess.run(["taskkill", "/PID", str(p.pid)], capture_output=True)
        time.sleep(2)
    print("已关闭，退出码 =", p.poll())
    return 0 if ok else 1

if __name__ == "__main__":
    sys.exit(main())
