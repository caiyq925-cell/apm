# -*- coding: utf-8 -*-
# Read-only diagnosis of the login window page state (LNA permission, quick-login).
import json
import sys

sys.path.insert(0, r"D:\Users\ai_apm\apm-monitor\tools")
from _cdp import targets, CDP  # noqa: E402

OUT = []

def note(*a):
    OUT.append(" ".join(str(x) for x in a))

PAGE_JS = r"""
(async function(){
  const out = {href: location.href, title: document.title};
  try { out.lna = (await navigator.permissions.query({name:'local-network-access'})).state; }
  catch(e){ out.lna = 'ERR: ' + e.message; }
  const warn = /本地网络访问权限|快捷登录/;
  out.warnTexts = [];
  document.querySelectorAll('p,div,span').forEach(el => {
    const t = (el.innerText||'').replace(/\s+/g,'');
    if (t && warn.test(t) && t.length < 80 && el.children.length < 3) out.warnTexts.push(t);
  });
  out.warnTexts = out.warnTexts.slice(0,5);
  const ifr = document.querySelector('iframe[src*="qrconnect"]');
  out.hasQrIframe = !!ifr;
  out.iframeAllow = ifr ? (ifr.getAttribute('allow')||'') : null;
  return JSON.stringify(out);
})()
"""

IFRAME_JS = r"""
(function(){
  const out = {href: location.href.slice(0,80)};
  const bodyText = (document.body ? document.body.innerText : '').replace(/\s+/g,'');
  out.bodyText = bodyText.slice(0, 300);
  out.imgs = [];
  document.querySelectorAll('img').forEach(im => out.imgs.push((im.src||'').slice(0,80)));
  out.imgs = out.imgs.slice(0,6);
  const kw = /快捷|扫码|确认|头像|登录/;
  out.kwHits = [];
  document.querySelectorAll('button, a, div, span, p').forEach(el => {
    const t = (el.innerText||'').replace(/\s+/g,'');
    if (t && kw.test(t) && t.length < 40 && el.children.length < 3) out.kwHits.push(t);
  });
  out.kwHits = [...new Set(out.kwHits)].slice(0,10);
  return JSON.stringify(out);
})()
"""

def main():
    ts = targets(9223)
    if not ts:
        note("no targets on 9223")
        return
    page = None
    iframe = None
    for t in ts:
        u = t.get("url") or ""
        if t.get("type") == "page" and "cloud.tencent.com/login" in u:
            page = t
        if t.get("type") == "iframe" and "qrconnect" in u:
            iframe = t
    note("page target:", bool(page), "iframe target:", bool(iframe))
    if page:
        c = CDP(page["webSocketDebuggerUrl"])
        try:
            note("PAGE:", c.evaluate(PAGE_JS))
        finally:
            c.close()
    if iframe:
        c2 = CDP(iframe["webSocketDebuggerUrl"])
        try:
            note("IFRAME:", c2.evaluate(IFRAME_JS))
        finally:
            c2.close()

main()
print("\n".join(OUT).encode("utf-8", "replace").decode("utf-8"))
