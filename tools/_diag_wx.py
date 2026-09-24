# -*- coding: utf-8 -*-
# Replicate the qrconnect page's exact WeChat detection probe from inside the
# iframe: POST https://localhost.weixin.qq.com:<port>/api/check-login
import sys

sys.path.insert(0, r"D:\Users\ai_apm\apm-monitor\tools")
from _cdp import targets, CDP  # noqa: E402

JS = r"""
(async function(){
  const ports = [14013,14014,14015,13013,13014,13015];
  const out = [];
  for (const p of ports) {
    const url = 'https://localhost.weixin.qq.com:' + p + '/api/check-login';
    const t0 = Date.now();
    try {
      const r = await fetch(url, {
        method: 'POST',
        headers: {'Content-Type': 'application/json'},
        body: JSON.stringify({apiname:'qrconnectchecklogin', jsdata:{appid:'wxca396cd1df083b9d'}}),
      });
      const txt = (await r.text()).slice(0, 120);
      out.push(p + ' -> HTTP ' + r.status + ' ' + txt);
    } catch(e) {
      out.push(p + ' -> ERR: ' + e.message + ' (' + (Date.now()-t0) + 'ms)');
    }
  }
  return JSON.stringify(out);
})()
"""

def main():
    ts = targets(9223)
    iframe = None
    for t in ts:
        if t.get("type") == "iframe" and "qrconnect" in (t.get("url") or ""):
            iframe = t
    if not iframe:
        print("no iframe target")
        return
    c = CDP(iframe["webSocketDebuggerUrl"], timeout=40)
    try:
        print(c.evaluate(JS, await_promise=True, timeout=60))
    finally:
        c.close()

main()
