"""验证"WebView2 数据目录被强杀弄坏 → 界面全白"这个推断，并给出修复。

步骤：清掉坏目录 → 启动 → 检查渲染 → **正常关闭**（WM_CLOSE，不是强杀）→ 再启动 → 再检查。
两次都渲染正常，说明只要不用强杀，目录就不会坏。
"""
import ctypes
import os
import shutil
import subprocess
import sys
import time
from ctypes import wintypes

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import _cdp

u = ctypes.windll.user32
WM_CLOSE = 0x0010
EXE = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "dist", "apm-monitor.exe"))
BASE = os.path.join(os.environ["LOCALAPPDATA"], "com.local.apmmonitor")


def find_main_window(pid):
    hw = [None]

    def cb(h, l):
        p = wintypes.DWORD()
        u.GetWindowThreadProcessId(h, ctypes.byref(p))
        if p.value == pid and u.IsWindowVisible(h):
            r = wintypes.RECT()
            u.GetWindowRect(h, ctypes.byref(r))
            if r.right - r.left > 400 and r.bottom - r.top > 300:
                hw[0] = h
                return False
        return True

    u.EnumWindows(ctypes.WINFUNCTYPE(ctypes.c_bool, wintypes.HWND, wintypes.LPARAM)(cb), 0)
    return hw[0]


def check(pid, wait=4.0):
    """等 wait 秒后检查界面是否渲染（调试端口存活期很短，必须趁早查）。"""
    time.sleep(wait)
    t = _cdp.targets()
    pages = [x for x in t if x.get("type") == "page"]
    if not pages:
        return None, "拿不到页面目标"
    cdp = _cdp.CDP(pages[0]["webSocketDebuggerUrl"])
    try:
        cdp.cmd("Runtime.enable")
        href = cdp.evaluate("location.href")
        scripts = cdp.evaluate("document.scripts.length")
        body = cdp.evaluate("document.body ? document.body.innerHTML.length : -1")
        return (bool(scripts) and body and body > 0), "href=%s scripts=%s body=%s" % (href, scripts, body)
    except Exception as e:
        return None, "%s: %s" % (type(e).__name__, e)
    finally:
        cdp.close()


def cleanup_data_dirs():
    removed = []
    if os.path.isdir(BASE):
        for name in os.listdir(BASE):
            if name.startswith("EBWebView"):
                path = os.path.join(BASE, name)
                shutil.rmtree(path, ignore_errors=True)
                removed.append(name)
    return removed


def main():
    print("清理损坏的 WebView2 数据目录…")
    print("  已删除:", cleanup_data_dirs() or "(无)")

    for rnd in (1, 2):
        print()
        print("=== 第 %d 次启动 ===" % rnd)
        p = subprocess.Popen([EXE], cwd=os.path.dirname(EXE))
        ok, detail = check(p.pid)
        print("  渲染正常 =", ok, "|", detail)
        hw = find_main_window(p.pid)
        if hw:
            u.PostMessageW(hw, WM_CLOSE, 0, 0)   # 正常关闭，不强杀
            for _ in range(20):
                if p.poll() is not None:
                    break
                time.sleep(0.3)
            print("  正常关闭后退出码 =", p.poll())
        if p.poll() is None:
            subprocess.run(["taskkill", "/PID", str(p.pid), "/T", "/F"], capture_output=True)
            print("  （兜底强杀）")

    print()
    print("如果两次都 True，说明：包没问题，白屏是强杀导致数据目录损坏引起的。")
    return 0


if __name__ == "__main__":
    sys.exit(main())
