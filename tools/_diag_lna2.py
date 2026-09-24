# -*- coding: utf-8 -*-
# Control test: fetch a KNOWN-open local port (CDP 9223) and a known-closed port
# from inside the qrconnect iframe, to distinguish LNA-block from connection-refused.
import sys

sys.path.insert(0, r"D:\Users\ai_apm\apm-monitor\tools")
from _cdp import targets, CDP  # noqa: E402

JS = r"""
(async function(){
  const out = {};
  async function probe(url){
    const t0 = Date.now();
    try {
      const r = await fetch(url, {cache:'no-store'});
      const txt = (await r.text()).slice(0, 60);
      return 'HTTP ' + r.status + ' body=' + txt.replace(/\s+/g,' ');
    } catch(e) {
      return 'FETCH-ERR: ' + e.message + ' (' + (Date.now()-t0) + 'ms)';
    }
  }
  out.cdp_http   = await probe('http://127.0.0.1:9223/json/version');   // known open, plain HTTP
  out.closed     = await probe('http://127.0.0.1:9/');                  // discard port, closed
  out.wechat_http = await probe('http://127.0.0.1:14013/');             // wechat, plain http
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
