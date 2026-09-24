# -*- coding: utf-8 -*-
# Inside the qrconnect iframe: check LNA permission state and test whether a
# fetch to the local WeChat service (127.0.0.1:14013...) is allowed or blocked.
import sys

sys.path.insert(0, r"D:\Users\ai_apm\apm-monitor\tools")
from _cdp import targets, CDP  # noqa: E402

JS = r"""
(async function(){
  const out = {};
  try { out.lna = (await navigator.permissions.query({name:'local-network-access'})).state; }
  catch(e){ out.lna = 'ERR '+e.message; }
  out.probes = [];
  for (const port of [14013, 14016, 14019, 14022, 14023]) {
    for (const scheme of ['http', 'https']) {
      const url = scheme + '://127.0.0.1:' + port + '/';
      const t0 = Date.now();
      try {
        const r = await fetch(url, {method:'GET', cache:'no-store'});
        out.probes.push(url + ' -> HTTP ' + r.status);
      } catch(e) {
        out.probes.push(url + ' -> FETCH-ERR: ' + e.message + ' (' + (Date.now()-t0) + 'ms)');
      }
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
    c = CDP(iframe["webSocketDebuggerUrl"], timeout=30)
    try:
        print(c.evaluate(JS, await_promise=True, timeout=40))
    finally:
        c.close()

main()
