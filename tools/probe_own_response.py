"""决定性实验：控制台页面**自己发的** dashboard 请求，在 WebView2 会话里到底回什么 code。

排查主线（2026-09-23 晚）：页面内 fetch 也 1216，需要区分两种可能——
  A. 页面自己的请求在这个会话里就失败（问题在会话/环境，不在我们的组装）；
  B. 页面自己的请求成功，只是我们复制的还差一层（那就继续对齐）。

做法：用 CDP 后台开一个 Deployment 详情页，监听 Network 事件，等到它自己发
DescribeDashboardMetricData，再用 Network.getResponseBody 读响应体，打印 code 与数据条数。
页面随后销毁（background:true，不打扰用户）。

用法：应用要开着（登录窗口关着无所谓，只要 WebView2 会话里有登录态）。
    python tools/probe_own_response.py
"""
import json
import os
import re
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import _cdp
import _session

PORT = _cdp.DEFAULT_PORT


def get_browser_ws():
    import urllib.request
    with _cdp._NO_PROXY.open("http://127.0.0.1:%d/json/version" % PORT, timeout=3) as r:
        return json.load(r)["webSocketDebuggerUrl"]


def main():
    cfg, uin, owner = _session.session()
    cluster = cfg.get("containerCluster") or ""
    namespace = cfg.get("containerNamespace") or ""
    deploys = cfg.get("selectedDeployments") or []
    if not cluster or not deploys:
        print("config.json 里没有集群/工作负载——先在界面选好")
        return 1
    url = ("https://console.cloud.tencent.com/tke2/cluster/sub/detail/resource/deployment"
           "?rid=%s&clusterId=%s&resourceIns=%s&np=%s"
           % (cfg.get("regionId", 4), cluster, deploys[0], namespace))
    print("后台打开详情页：%s" % url[:120])

    ws = _cdp.WS(get_browser_ws(), timeout=25)
    nid = [0]

    def send(method, params=None, session=None):
        nid[0] += 1
        msg = {"id": nid[0], "method": method, "params": params or {}}
        if session:
            msg["sessionId"] = session
        ws.send_text(json.dumps(msg))
        return nid[0]

    def recv(timeout):
        ws.sock.settimeout(timeout)
        return json.loads(ws.recv_text())

    def wait_result(mid, timeout=25):
        deadline = time.time() + timeout
        while time.time() < deadline:
            m = recv(max(1.0, deadline - time.time()))
            if m.get("id") == mid:
                return m
        return None

    # 开后台页
    r = wait_result(send("Target.createTarget", {"url": url, "newWindow": True, "background": True}))
    target_id = ((r or {}).get("result") or {}).get("targetId")
    if not target_id:
        print("createTarget 失败：%s" % json.dumps(r, ensure_ascii=False)[:200])
        return 1
    r = wait_result(send("Target.attachToTarget", {"targetId": target_id, "flatten": True}))
    sid = ((r or {}).get("result") or {}).get("sessionId")
    if not sid:
        print("attach 失败")
        ws.close()
        return 1
    print("已挂上 target %s" % target_id[:8])

    send("Network.enable", session=sid)

    req_meta = {}   # requestId -> (cmd, csrf, x_lid, t_ms)
    got = {}        # requestId -> body text
    deadline = time.time() + 60
    print("等页面自己发 DescribeDashboardMetricData（最多 60s）…")
    while time.time() < deadline and len(got) < 1:
        try:
            m = recv(max(1.0, deadline - time.time()))
        except Exception as e:
            print("读事件失败：%s" % e)
            break
        if m.get("sessionId") != sid:
            continue
        meth = m.get("method")
        p = m.get("params") or {}
        if meth == "Network.requestWillBeSent":
            u = (p.get("request") or {}).get("url") or ""
            if "/cgi/capi" not in u:
                continue
            cm = re.search(r"cmd=(\w+)", u)
            cs = re.search(r"csrfCode=([A-Za-z0-9_-]+)", u)
            tm = re.search(r"[?&]t=(\d+)", u)
            hdrs = (p.get("request") or {}).get("headers") or {}
            lid = ""
            life = ""
            for k, v in hdrs.items():
                if k.lower() == "x-lid":
                    lid = str(v)
                if k.lower() == "x-life":
                    life = str(v)
            req_meta[p.get("requestId")] = {
                "cmd": cm.group(1) if cm else "",
                "csrf": cs.group(1) if cs else "",
                "x_lid": lid, "x_life": life,
                "t": int(tm.group(1)) if tm else 0,
            }
        elif meth == "Network.loadingFinished":
            rid = p.get("requestId")
            if rid in req_meta and req_meta[rid]["cmd"] == "DescribeDashboardMetricData":
                rb = wait_result(send("Network.getResponseBody", {"requestId": rid}, session=sid))
                body = ((rb or {}).get("result") or {}).get("body", "")
                got[rid] = body
                break

    # 打印结论
    for rid, body in got.items():
        meta = req_meta[rid]
        epoch = (meta["t"] - int(meta["x_life"])) if (meta["t"] and meta["x_life"].isdigit()) else 0
        print("\n=== 页面自己发的 DescribeDashboardMetricData ===")
        print("  csrf=%s…(len=%d)  x-lid=%s…  x-life=%s  life_epoch=%d"
              % (meta["csrf"][:6], len(meta["csrf"]), meta["x_lid"][:6], meta["x_life"], epoch))
        try:
            v = json.loads(body)
            d = ((v.get("data") or {}).get("data") or {}).get("Response") or {}
            arr = d.get("Data") or []
            print("  code=%s msg=%s  Data=%d 条" % (v.get("code"), v.get("msg"), len(arr)))
            if arr:
                print("  指标=%s 值样本=%s" % (arr[0].get("MetricName"), str(arr[0].get("Value"))[:60]))
        except Exception:
            print("  响应非 JSON：%s" % body[:160])

    if not got:
        print("60 秒内页面没发 DescribeDashboardMetricData（可能页面没加载出来/被重定向到登录页）")

    # 收尾：关掉这个后台页
    send("Target.closeTarget", {"targetId": target_id})
    ws.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
