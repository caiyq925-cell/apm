"""在登录窗口的浏览器里，用"页面自己那份令牌"重放一次旧网关 dashboard 请求。

结论用途（对应 docs/HANDOFF-会话与登录改造.md 第二条 step 3）：
  - 页面自己的令牌 -> code=0 且有数据  => "页面令牌 + 我们的请求组装"可行
  - 页面自己的令牌也 1216             => 令牌/头的对齐还有问题，别怀疑"谁发"

**必须带 x-lid / x-life 两个头**（2026-09-23 抓包定论，见 HANDOFF 第二条）：
控制台页面自己发 `/cgi/capi` 请求时必带这两个头，缺了它们——脚本直发回 `code=1216`，
页面里 fetch 回 `code=-1`。本脚本先监听 CDP Network 事件，从页面自己发的请求里抓
`x-lid` 与 `x-life` 的会话基准 `life_epoch`（= 该请求的 t_ms - 该请求的 x-life），
再带进 fetch；`x-life` 现场生成 `Date.now() - life_epoch`（复用抓到的旧值会被网关拒）。

用法（先在界面上点开「扫码登录」，确保有腾讯域名的页面在跑）：
    python tools/capi_in_browser.py

不再依赖外部的 _replay_body.json：请求体由 _capi.dashboard_body 按 config.json 自己拼
（与 container.rs::pod_util_metrics 同构）。Pod 列表尽力从 /_api 取，取不到就退化成
"只按 region+cluster" —— 这已经足够回答"令牌到底被不被接受"。
"""
import json
import os
import re
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import _capi
import _cdp
import _session

PORT = _cdp.DEFAULT_PORT

JS = r"""
(async () => {
  const out = [];
  const base = "https://console.cloud.tencent.com/cgi/capi?cmd=DescribeDashboardMetricData&action=delegate"
    + "&serviceType=monitor&secure=1&version=3&dictId=2006&sts=1&t=" + Date.now()
    + "&uin=__UIN__&ownerUin=__OWN__&csrfCode=";
  // x-lid 用页面自己那份；x-life 按同一公式现场生成（页面自己也是这么发的）
  const hdrs = {"Content-Type": "application/json", "X-Requested-With": "XMLHttpRequest",
                "x-lid": "__XLID__", "x-life": String(Date.now() - __LIFE_EPOCH__)};
  for (const [tag, csrf] of [["页面API令牌", "__API_CSRF__"], ["配置里的令牌", "__CFG__"]]) {
    if (!csrf) { out.push(tag + "：无值，跳过"); continue; }
    try {
      const r = await fetch(base + csrf, {
        method: "POST", credentials: "include", headers: hdrs, body: __BODY__
      });
      const txt = await r.text();
      let brief;
      try {
        const v = JSON.parse(txt);
        const d = ((v.data || {}).data || {}).Response || {};
        const arr = d.Data || [];
        brief = "code=" + v.code + " msg=" + (v.msg || "") + " Data=" + arr.length
              + (arr[0] ? " 值样本=" + String(arr[0].Value).slice(0, 40) : "");
      } catch (e) { brief = txt.slice(0, 140); }
      out.push(tag + " -> " + brief);
    } catch (e) { out.push(tag + " -> fetch 失败 " + e); }
  }
  return out.join("\n");
})()
"""


def hdr(headers, key):
    for k, v in (headers or {}).items():
        if k.lower() == key:
            return str(v)
    return ""


def capture_capi_meta(cdp, timeout=25):
    """监听 Network 事件，等页面自己发 /cgi/capi 请求，抓 (csrfCode, x-lid, life_epoch)。

    life_epoch = 该请求的 t_ms - 该请求的 x-life（会话常数）。等不到返回全空。
    """
    deadline = time.time() + timeout
    while time.time() < deadline:
        ev = cdp.wait_event("Network.requestWillBeSent", timeout=5)
        if not ev:
            continue
        req = (ev.get("params") or {}).get("request") or {}
        url = req.get("url") or ""
        if "/cgi/capi" not in url or "csrfCode=" not in url:
            continue
        m = re.search(r"csrfCode=([A-Za-z0-9_-]+)", url)
        tm = re.search(r"[?&]t=(\d+)", url)
        lid = hdr(req.get("headers"), "x-lid")
        life = hdr(req.get("headers"), "x-life")
        t = int(tm.group(1)) if tm else 0
        epoch = (t - int(life)) if (t and life.isdigit()) else 0
        return ((m.group(1) if m else ""), lid, epoch)
    return ("", "", 0)


def fetch_pods(cfg, uin, owner, cluster, namespace, deployment):
    """尽力取该 Deployment 的 Pod 名；失败返回空列表。"""
    path = "/apis/apps/v1/namespaces/%s/deployments/%s/pods?limit=500" % (namespace, deployment)
    raw = _capi.hc(cfg, uin, owner, "tke", "ForwardPlatformRequestV3", {
        "Version": "2018-05-25", "Language": "zh-CN", "Method": "GET",
        "Path": path, "ClusterName": cluster,
    }, "取 Pod 列表", referer=_capi.CAPI_REFERER, timeout=30)
    if not raw:
        return []
    try:
        resp = json.loads(raw).get("Response") or {}
        items = json.loads(resp.get("ResponseBody") or "{}").get("items", [])
        return [p["metadata"]["name"] for p in items]
    except Exception as e:
        print("（解析 Pod 列表失败：%s）" % e)
        return []


def main():
    cfg, uin, owner = _session.session()
    cluster = cfg.get("containerCluster") or ""
    namespace = cfg.get("containerNamespace") or ""
    deploys = cfg.get("selectedDeployments") or []
    if not cluster:
        print("config.json 里没有 containerCluster —— 先在界面选好集群/命名空间/工作负载")
        return 1

    end = int(time.time())
    start = end - 3600
    pods = []
    if deploys and namespace:
        print("取 Pod 列表：%s / %s / %s" % (cluster, namespace, deploys[0]))
        pods = fetch_pods(cfg, uin, owner, cluster, namespace, deploys[0])
        print("  Pod 数：%d" % len(pods))

    body_data = _capi.dashboard_body(cfg, cluster, namespace, deploys[0] if deploys else "", pods, start, end)
    # 旧网关请求体 = 内层 JSON **直发**（{"cmd":..,"serviceType":..,"data":{..},"regionId":4}）。
    # 抓包 content-length 实锤：没有 {"text":"..."} 包装；包装了网关解析不到顶层 cmd 回 1216
    body = json.dumps({"cmd": "DescribeDashboardMetricData", "serviceType": "monitor",
                       "data": body_data, "regionId": cfg.get("regionId", 4)}, ensure_ascii=False)

    print("找登录窗口页面（127.0.0.1:%d）…" % PORT)
    page = _cdp.find_page(PORT)
    if not page:
        print("没找到腾讯域名的页面目标 —— 先在界面上点「扫码登录」，让登录窗口开着")
        return 1
    print("页面：%s" % (page.get("url") or "")[:100])

    cdp = _cdp.CDP(page["webSocketDebuggerUrl"])
    try:
        cdp.cmd("Runtime.enable")
        cdp.cmd("Network.enable")
        print("等页面自己发 /cgi/capi 请求，抓 x-lid / x-life 基准…")
        api_csrf, x_lid, life_epoch = capture_capi_meta(cdp)
        if not api_csrf or not x_lid or not life_epoch:
            print("  ⚠️ 没抓到（csrf=%s x-lid=%s life_epoch=%s）—— 登录窗口可能不在 TKE 页面上，"
                  "先到容器服务页签选中集群/命名空间/工作负载" % (bool(api_csrf), bool(x_lid), bool(life_epoch)))
        else:
            print("  页面 API 令牌: %s…(len=%d)  x-lid: %s…  life_epoch: %s"
                  % (api_csrf[:6], len(api_csrf), x_lid[:6], life_epoch))
        js = (JS.replace("__UIN__", uin).replace("__OWN__", owner)
              .replace("__API_CSRF__", api_csrf)
              .replace("__XLID__", x_lid)
              .replace("__LIFE_EPOCH__", str(life_epoch))
              .replace("__CFG__", cfg["csrfCode"])
              .replace("__BODY__", json.dumps(body)))
        print(cdp.evaluate(js))
    finally:
        cdp.close()

    print("\n判定：")
    print("  页面API令牌 code=0 且有数据 -> 令牌口径 + x-lid/x-life 已对齐，容器链路应该通了")
    print("  页面API令牌仍 1216/-1       -> 还有没对齐的字段，按抓包逐项核")
    return 0


if __name__ == "__main__":
    sys.exit(main())
