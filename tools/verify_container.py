"""一条命令验证"容器 CPU/内存利用率"这条链路。

它做三件事：
  1. 找到正在运行的应用主窗口；
  2. 用 UIA 点一次「开始统计」（不用你手动点）；
  3. 盯 dist/apm-monitor.log，把这次查询新增的日志摘出来，并给出结论。

前置条件：
  - 应用**已经打开**（并且已经点过「扫码登录」完成扫码 —— 预热窗口要靠浏览器会话去访问控制台页）；
  - 用的是带日志落盘的版本（dist/apm-monitor.log 存在）。

用法：
    python tools/verify_container.py
"""
import ctypes
import os
import re
import subprocess
import sys
import time
from ctypes import wintypes

u = ctypes.windll.user32
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LOG = os.path.join(ROOT, "dist", "apm-monitor.log")
TOOLS = os.path.join(ROOT, "tools")


def app_pids():
    out = subprocess.run('tasklist /FI "IMAGENAME eq apm-monitor.exe" /FO CSV /NH',
                         shell=True, capture_output=True).stdout.decode("utf-8", "replace")
    pids = []
    for line in out.splitlines():
        m = re.match(r'"[^"]+","(\d+)"', line.strip())
        if m:
            pids.append(int(m.group(1)))
    return pids


def main_window(pid):
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


def tail_from(path, offset):
    try:
        with open(path, encoding="utf-8", errors="replace") as f:
            f.seek(offset)
            return f.read()
    except Exception:
        return ""


def main():
    pids = app_pids()
    if not pids:
        print("应用没在运行。请先打开 dist/apm-monitor.exe，")
        print("并在设置里点一次「扫码登录」完成扫码（预热窗口需要浏览器会话已登录）。")
        return 1
    if not os.path.exists(LOG):
        print("找不到 %s —— 说明当前跑的不是带日志落盘的版本。" % LOG)
        print("请用最新构建（见 docs/HANDOFF-会话与登录改造.md 第九节）。")
        return 1

    pid = pids[0]
    hwnd = main_window(pid)
    print("应用 PID=%s，主窗口 HWND=%s" % (pid, hwnd))
    if not hwnd:
        print("找不到主窗口（可能界面白屏 —— 见文档坑九：删掉 "
              "%LOCALAPPDATA%\\com.local.apmmonitor\\EBWebView 后重启）")
        return 1

    offset = os.path.getsize(LOG)
    print("记录日志起点 offset=%d，点击「开始统计」…" % offset)
    r = subprocess.run(["powershell", "-ExecutionPolicy", "Bypass", "-File",
                        os.path.join(TOOLS, "startquery.ps1"), "-Hwnd", str(hwnd)],
                       capture_output=True)
    out = (r.stdout or b"").decode("utf-8", "replace").strip()
    print("  " + (out or "(startquery.ps1 无输出)"))
    if "invoked" not in out:
        print("  !! 没能点到「开始统计」，请手动点一次；脚本继续盯日志 3 分钟。")

    print()
    print("等待查询结束（最多 4 分钟）…")
    new = ""
    deadline = time.time() + 240
    while time.time() < deadline:
        time.sleep(3)
        new = tail_from(LOG, offset)
        if "统计完成" in new or "统计中断" in new or "容器指标查询失败" in new:
            # 再多等两拍，把后续行收全
            time.sleep(6)
            new = tail_from(LOG, offset)
            break

    print()
    print("=== 本次查询新增的日志 ===")
    print(new.strip() or "(没有新增日志 —— 查询可能没真正开始)")

    print()
    print("=== 结论 ===")
    checks = [
        ("容器指标令牌预热成功", "预热拿到了令牌（含浏览器 Cookie）"),
        ("容器指标令牌预热失败", "预热失败 —— 先点一次「扫码登录」再试"),
        ("容器指标查询失败: 旧网关接口错误 code=1216", "脚本直发被网关拒（应会自动走退路）"),
        ("退路也失败", "连隐藏页面自己发都不行 —— 不是「谁发」的问题"),
        ("cpu_util_limit", "出现利用率指标 —— 成功"),
        ("统计完成", "整次统计跑完了"),
        ("统计中断", "统计被中断（不该因容器失败而中断）"),
    ]
    hit_any = False
    for key, desc in checks:
        if key in new:
            print("  [命中] %s  ->  %s" % (key, desc))
            hit_any = True
    if not hit_any:
        print("  以上关键标记都没出现 —— 请把上面的日志贴给我。")
    print()
    print("（日志文件：%s）" % LOG)
    return 0


if __name__ == "__main__":
    sys.exit(main())
