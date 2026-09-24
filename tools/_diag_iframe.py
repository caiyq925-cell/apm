# -*- coding: utf-8 -*-
# Check which UI is VISIBLE inside the qrconnect iframe: QR code or quick-login panel.
import json
import sys

sys.path.insert(0, r"D:\Users\ai_apm\apm-monitor\tools")
from _cdp import targets, CDP  # noqa: E402

JS = r"""
(function(){
  function vis(el){
    if(!el) return false;
    const r = el.getBoundingClientRect();
    const cs = getComputedStyle(el);
    return r.width>0 && r.height>0 && cs.display!=='none' && cs.visibility!=='hidden';
  }
  const out = {};
  // images: qr vs avatar
  out.images = [];
  document.querySelectorAll('img').forEach(im=>{
    out.images.push({src:(im.src||'').split('/').pop().slice(0,40), visible: vis(im),
                     w: Math.round(im.getBoundingClientRect().width)});
  });
  // candidate quick-login buttons/links
  out.quick = [];
  const kw = /快捷登录|一键登录|免扫码/;
  document.querySelectorAll('button, a, [role=button], div, span, p, li').forEach(el=>{
    const t = (el.innerText||'').replace(/\s+/g,'');
    if(!t || !kw.test(t)) return;
    if(t.length>30) return;
    out.quick.push({tag: el.tagName, cls:(el.className||'').toString().slice(0,40),
                    text: t.slice(0,24), visible: vis(el)});
  });
  out.quick = out.quick.slice(0,12);
  // any visible element containing 扫一扫 (QR hint)
  out.scanHintVisible = false;
  document.querySelectorAll('div,span,p').forEach(el=>{
    const t=(el.innerText||'').replace(/\s+/g,'');
    if(t && t.includes('扫一扫') && t.length<30 && vis(el)) out.scanHintVisible = true;
  });
  // nickname / avatar text near quick login
  out.nickVisible = [];
  document.querySelectorAll('div,span,p').forEach(el=>{
    const t=(el.innerText||'').replace(/\s+/g,'');
    if(t && (t.includes('微信用户')||t.includes('确认')) && t.length<40 && vis(el)) out.nickVisible.push(t.slice(0,30));
  });
  out.nickVisible = [...new Set(out.nickVisible)].slice(0,6);
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
    c = CDP(iframe["webSocketDebuggerUrl"])
    try:
        print(c.evaluate(JS))
    finally:
        c.close()

main()
