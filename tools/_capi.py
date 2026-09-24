"""控制台两条链路的薄封装（诊断脚本共用）。

只做"发一次请求 + 打印耗时与 code"，不做业务解析 —— 诊断脚本关心的是
"这条路通不通、耗时多少、错误文案是什么"。

两条链路的令牌互不通用，别混用（详见 docs/DESIGN.md 第四节）。
"""
import datetime
import json
import time
import urllib.error
import urllib.request

import _session

HC_BASE = "https://console-hc.cloud.tencent.com/_api"
CAPI_BASE = "https://console.cloud.tencent.com/cgi/capi"

HC_REFERER = "https://console.cloud.tencent.com/monitor/apm/system/list"
CAPI_REFERER = "https://console.cloud.tencent.com/tke2/cluster"


def _iso_local(ts):
    return datetime.datetime.fromtimestamp(
        ts, datetime.timezone(datetime.timedelta(hours=8))).strftime("%Y-%m-%dT%H:%M:%S+08:00")


def _post(url, body, cfg, label, referer, timeout):
    req = urllib.request.Request(url, data=body, headers=_session.headers(cfg["cookie"], referer))
    t0 = time.time()
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            raw = r.read().decode("utf-8", "replace")
            dt = time.time() - t0
            code = msg = None
            try:
                v = json.loads(raw)
                code, msg = v.get("code"), v.get("msg")
            except Exception:
                pass
            print("[%s] %6.1fs HTTP=%s code=%s len=%d" % (label, dt, r.status, code, len(raw)))
            if msg:
                print("        msg: %s" % str(msg)[:160])
            return raw
    except urllib.error.HTTPError as e:
        print("[%s] %6.1fs HTTPError %s %s" % (label, time.time() - t0, e.code, e.reason))
    except Exception as e:
        print("[%s] %6.1fs %s: %s" % (label, time.time() - t0, type(e).__name__, e))
    return None


def hc(cfg, uin, owner, service, action, data, label, referer=HC_REFERER, timeout=120):
    """新网关 console-hc.cloud.tencent.com/_api/{service}/{Action}"""
    now = int(time.time() * 1000)
    url = ("%s/%s/%s?timeout=30000&t=%d&uin=%s&ownerUin=%s&csrfCode=%s"
           % (HC_BASE, service, action, now, uin, owner, cfg["csrfCode"]))
    body = json.dumps({"cmd": action, "serviceType": service, "data": data,
                       "regionId": cfg.get("regionId", 4)}).encode()
    return _post(url, body, cfg, label, referer, timeout)


def capi(cfg, uin, owner, service, cmd, data, label, referer=CAPI_REFERER, timeout=120):
    """旧网关 console.cloud.tencent.com/cgi/capi（容器利用率唯一数据源）

    请求体 = 内层 JSON **直接发**（{"cmd":..,"serviceType":..,"data":{..},"regionId":4}）。
    抓包 content-length 实锤：控制台的 /cgi/capi 请求体就是这个，没有 {"text":"..."} 包装——
    包装了网关解析不到顶层 cmd，回 code=1216 不合法的云 API 类型。
    """
    body = json.dumps({"cmd": cmd, "serviceType": service, "data": data,
                       "regionId": cfg.get("regionId", 4)}, ensure_ascii=False).encode()
    now = int(time.time() * 1000)
    # 注意：URL 里**不能**带 json=1 —— 控制台自己的请求没有它，加了会被判成另一种云 API 类型
    url = ("%s?cmd=%s&action=delegate&serviceType=%s&secure=1&version=3&dictId=2006&sts=1"
           "&t=%d&uin=%s&ownerUin=%s&csrfCode=%s"
           % (CAPI_BASE, cmd, service, now, uin, owner, cfg["csrfCode"]))
    return _post(url, body, cfg, label, referer, timeout)


def pick_period(start, end):
    """与 container.rs::pick_period 一致（QCE 只接受标准周期）。"""
    secs = max(0, end - start)
    if secs <= 3600:
        return 60
    if secs <= 6 * 3600:
        return 300
    if secs <= 24 * 3600:
        return 3600
    return 86400


def dashboard_body(cfg, cluster, namespace, workload, pods, start, end):
    """旧网关 dashboard 容器利用率请求体（与 container.rs::pod_util_metrics 同构）。

    维度**必须**和控制台页面自己的请求一致：region / tke_cluster_instance_id /
    pod_name / namespace / workload_name，少一个都可能取不到数据（抓包核过）。
    """
    dims = [{"Key": "region", "Value": [cfg["region"]], "Operator": "eq"},
            {"Key": "tke_cluster_instance_id", "Value": [cluster], "Operator": "in"}]
    if pods:
        dims.append({"Key": "pod_name", "Value": pods, "Operator": "in"})
    if namespace:
        dims.append({"Key": "namespace", "Value": [namespace], "Operator": "eq"})
    if workload:
        dims.append({"Key": "workload_name", "Value": [workload], "Operator": "eq"})
    dim_str = json.dumps(dims, ensure_ascii=False)
    period = pick_period(start, end)
    return {
        "Version": "2018-07-24", "Language": "zh-CN", "SpaceUUID": "space_default",
        "Module": "monitor",
        "Query": [{
            "Datasource": "DS_QCEMetric", "Namespace": "QCE/TKE2", "MetricName": metric,
            "Conditions": [{"Region": cfg["region"], "Dimension": [dim_str]}],
            "GroupBy": ["InstanceId"],
            "StartTime": _iso_local(start), "EndTime": _iso_local(end),
            "Period": period, "QueryVersion": "2020-10-21",
        } for metric in ("K8sPodRateCpuCoreUsedLimit", "K8sPodRateMemNoCacheLimit")],
    }


def iso_local(ts):
    return _iso_local(ts)
