# -*- coding: utf-8 -*-
# Try candidate CDP permission names for Local Network Access.
import json
import sys
import urllib.request

sys.path.insert(0, r"D:\Users\ai_apm\apm-monitor\tools")
from _cdp import CDP  # noqa: E402

_NO_PROXY = urllib.request.build_opener(urllib.request.ProxyHandler({}))

def browser_ws(port=9223):
    with _NO_PROXY.open("http://127.0.0.1:%d/json/version" % port, timeout=3) as r:
        return json.load(r)["webSocketDebuggerUrl"]

NAMES = [
    "localNetworkAccess",
    "local-network-access",
    "privateNetworkAccess",
    "private-network-access",
    "localNetwork",
    "loopbackNetworkAccess",
]

def main():
    c = CDP(browser_ws(), timeout=15)
    try:
        for name in NAMES:
            resp = c.cmd("Browser.setPermission", {
                "permission": {"name": name},
                "setting": "granted",
                "origin": "https://open.weixin.qq.com",
            })
            err = resp.get("error")
            print(name, "->", ("ERROR: " + err.get("message", "")) if err else "OK granted")
    finally:
        c.close()

main()
