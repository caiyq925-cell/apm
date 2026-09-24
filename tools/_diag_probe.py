# -*- coding: utf-8 -*-
# Capture the qrconnect iframe's network requests during reload.
import json
import sys
import time

sys.path.insert(0, r"D:\Users\ai_apm\apm-monitor\tools")
from _cdp import targets, CDP  # noqa: E402

def find_iframe(tries=60, delay=0.5):
    for _ in range(tries):
        for t in targets(9223):
            if t.get("type") == "iframe" and "qrconnect" in (t.get("url") or ""):
                return t
        time.sleep(delay)
    return None

def drain(c, seconds):
    seen = []
    end = time.time() + seconds
    while time.time() < end:
        try:
            c.ws.sock.settimeout(2)
            msg = json.loads(c.ws.recv_text())
        except Exception:
            continue
        if msg.get("method") == "Network.requestWillBeSent":
            url = ((msg.get("params") or {}).get("request") or {}).get("url") or ""
            seen.append(url)
    return seen

def main():
    iframe = find_iframe()
    if not iframe:
        print("no iframe target (timeout)")
        return
    print("iframe found")
    c = CDP(iframe["webSocketDebuggerUrl"], timeout=20)
    try:
        c.cmd("Network.enable")
        try:
            c.cmd("Page.reload", {"ignoreCache": True}, timeout=8)
            print("reloaded")
        except Exception as e:
            print("reload err:", e)
        seen = drain(c, 20)
        print("TOTAL", len(seen))
        local = [u for u in seen if any(k in u for k in ("127.0.0.1", "localhost", "local", ".weixin", ":6", ":8", ":43"))]
        print("== LOCAL-LIKE ==")
        for u in dict.fromkeys(local):
            print("  ", u[:170])
        print("== ALL (first 30) ==")
        for u in seen[:30]:
            print("  ", u[:170])
    finally:
        c.close()

main()
