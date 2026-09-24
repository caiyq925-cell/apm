"""数据库 / APM 链路耗时探针（全部走 /_api 新网关）。

用途：区分"接口慢"和"接口报错"，并核对指标口径是否还成立。

注意口径（与 docs/DESIGN.md 第五节一致，别再用旧名字）：
  - MongoDB 命名空间是 QCE/CMONGO（不是 QCE/MONGODB），维度名 target（小写）
  - Redis 命名空间是 QCE/REDIS_MEM（QCE/REDIS 已下线），维度名 instanceid（小写）

用法：python tools/time_probe.py
"""
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import _capi
import _session


def main():
    cfg, uin, owner = _session.session()
    end = int(time.time())
    start = end - 3600
    iso = _capi.iso_local

    mongo_ids = (cfg.get("selectedDbInstances") or {}).get("mongodb", [])
    redis_ids = (cfg.get("selectedDbInstances") or {}).get("redis", [])
    print("mongo: %s\nredis: %s" % (mongo_ids, redis_ids))

    def monitor(ns, metric, dim_name, dim_value, label):
        return _capi.hc(cfg, uin, owner, "monitor", "GetMonitorData", {
            "Version": "2018-07-24", "Language": "zh-CN",
            "Namespace": ns, "MetricName": metric,
            "Instances": [{"Dimensions": [{"Name": dim_name, "Value": dim_value}]}],
            "Period": 60, "StartTime": iso(start), "EndTime": iso(end),
        }, label)

    # 1) MongoDB
    if mongo_ids:
        monitor("QCE/CMONGO", "MonogdMaxCpuUsage", "target", mongo_ids[0], "mongo:cpu")
        monitor("QCE/CMONGO", "MongodMaxMemUsage", "target", mongo_ids[0], "mongo:mem")

    # 2) Redis
    if redis_ids:
        monitor("QCE/REDIS_MEM", "CpuUtil", "instanceid", redis_ids[0], "redis:cpu")
        monitor("QCE/REDIS_MEM", "MemUtil", "instanceid", redis_ids[0], "redis:mem")

    # 3) 实例列表（query_db_metrics 每次都会先调它）
    _capi.hc(cfg, uin, owner, "dbbrain", "DescribeDiagDBInstances", {
        "Version": "2019-10-16", "Language": "zh-CN",
        "Product": "mongodb", "IsSupported": True, "Limit": 100, "Offset": 0,
    }, "dbbrain-list")

    # 4) APM 应用列表（不带时间参数，一次返回全部）
    if cfg.get("instanceId"):
        _capi.hc(cfg, uin, owner, "apm", "DescribeApmServiceMetric", {
            "InstanceId": cfg["instanceId"], "PageSize": 10,
        }, "apm:apps")
    else:
        print("（config.json 没有 instanceId，跳过 APM）")

    print("done")
    return 0


if __name__ == "__main__":
    sys.exit(main())
