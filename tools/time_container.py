"""容器链路耗时探针：/_api 的 K8s 资源列表 + 旧网关 dashboard 利用率。

用途：
  - 看容器那一步慢在哪（资源列表 vs 利用率）
  - 看旧网关到底回什么：`code=0`（通）还是 `code=1216`（令牌没被接受）

用法：python tools/time_container.py
"""
import json
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import _capi
import _session


def main():
    cfg, uin, owner = _session.session()
    cluster = cfg.get("containerCluster") or ""
    namespace = cfg.get("containerNamespace") or ""
    deploys = cfg.get("selectedDeployments") or []
    if not cluster or not namespace or not deploys:
        print("config.json 里缺 containerCluster / containerNamespace / selectedDeployments —— 先在界面选好")
        return 1

    print("region=%s regionId=%s cluster=%s ns=%s deploys=%s"
          % (cfg["region"], cfg.get("regionId"), cluster, namespace, deploys))

    # 1) Deployment 列表
    raw = _capi.hc(cfg, uin, owner, "tke", "ForwardPlatformRequestV3", {
        "Version": "2018-05-25", "Language": "zh-CN", "Method": "GET",
        "Path": "/apis/apps/v1/namespaces/%s/deployments?limit=500" % namespace,
        "ClusterName": cluster,
    }, "A:deploy-list")
    if raw:
        try:
            items = json.loads(json.loads(raw)["Response"]["ResponseBody"]).get("items", [])
            print("    deployments: %d" % len(items))
        except Exception as e:
            print("    parse: %s %s" % (e, str(raw)[:200]))

    # 2) 第一个工作负载的 Pod 列表
    dep = deploys[0]
    pods = []
    raw = _capi.hc(cfg, uin, owner, "tke", "ForwardPlatformRequestV3", {
        "Version": "2018-05-25", "Language": "zh-CN", "Method": "GET",
        "Path": "/apis/apps/v1/namespaces/%s/deployments/%s/pods?limit=500" % (namespace, dep),
        "ClusterName": cluster,
    }, "B:pods")
    if raw:
        try:
            items = json.loads(json.loads(raw)["Response"]["ResponseBody"]).get("items", [])
            pods = [p["metadata"]["name"] for p in items]
            print("    pods: %d %s" % (len(pods), pods[:3]))
        except Exception as e:
            print("    parse: %s %s" % (e, str(raw)[:200]))

    # 3) 旧网关 dashboard 利用率 —— 带 pod 过滤 / 不带 pod 过滤各打一次
    end = int(time.time())
    start = end - 3600
    for tag, use_pods in (("C:capi+pods", pods), ("D:capi-no-pods", [])):
        raw = _capi.capi(cfg, uin, owner, "monitor", "DescribeDashboardMetricData",
                         _capi.dashboard_body(cfg, cluster, namespace, dep, use_pods, start, end), tag)
        if not raw:
            continue
        try:
            v = json.loads(raw)
            d = ((v.get("data") or {}).get("data") or {}).get("Response") or {}
            arr = d.get("Data") or []
            print("    Data=%d %s" % (len(arr), [
                {"MetricName": x.get("MetricName"), "Value": str(x.get("Value"))[:40]} for x in arr[:2]]))
        except Exception as e:
            print("    parse: %s %s" % (e, str(raw)[:200]))

    print("done")
    return 0


if __name__ == "__main__":
    sys.exit(main())
