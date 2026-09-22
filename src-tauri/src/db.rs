// 数据库监控（MySQL / Redis / MongoDB）
// 指标来源：云监控 GetMonitorData + DBbrain（磁盘/健康得分）
// 所有指标名均经控制台接口对真实实例实测确认
use crate::apm::Channel;
use serde_json::{json, Value};

fn iso_local(secs: i64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_opt(secs, 0)
        .single()
        .map(|d| d.format("%Y-%m-%dT%H:%M:%S%:z").to_string())
        .unwrap_or_default()
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DbInstance {
    pub id: String,
    pub name: String,
}

pub async fn list_db_instances(ch: &Channel, db_type: &str) -> Result<Vec<DbInstance>, String> {
    // 优先用 DBbrain 的实例列表（与「数据库智能管家 → 实例管理」页面口径一致，覆盖更全：
    // 实测 MySQL 95 个，而 CDB 接口只回 49 个）
    match list_from_dbbrain(ch, db_type).await {
        Ok(list) if !list.is_empty() => Ok(list),
        Ok(_) => list_from_product_api(ch, db_type).await,
        Err(e) => match list_from_product_api(ch, db_type).await {
            Ok(list) if !list.is_empty() => Ok(list),
            _ => Err(e),
        },
    }
}

async fn list_from_dbbrain(ch: &Channel, db_type: &str) -> Result<Vec<DbInstance>, String> {
    let product = match db_type {
        "mysql" | "redis" | "mongodb" => db_type,
        _ => return Err(format!("不支持的数据库类型: {}", db_type)),
    };
    let mut out = Vec::new();
    let (page_size, max_pages) = (100, 30);
    for page in 0..max_pages {
        let payload = json!({
            "Version": "2019-10-16",
            "Product": product,
            "IsSupported": true,
            "Limit": page_size,
            "Offset": page * page_size
        });
        let resp = ch
            .call_service("dbbrain", "2019-10-16", "DescribeDiagDBInstances", &payload)
            .await?;
        let mut got = 0;
        if let Some(items) = resp["Items"].as_array() {
            for it in items {
                got += 1;
                let id = it["InstanceId"].as_str().unwrap_or("").to_string();
                if id.is_empty() {
                    continue;
                }
                let name = it["InstanceName"].as_str().unwrap_or("").to_string();
                if out.iter().any(|d: &DbInstance| d.id == id) {
                    continue;
                }
                out.push(DbInstance { id, name });
            }
        }
        if got < page_size {
            break;
        }
    }
    Ok(out)
}

async fn list_from_product_api(ch: &Channel, db_type: &str) -> Result<Vec<DbInstance>, String> {
    let (service, version, action, list_key) = match db_type {
        "mysql" => ("cdb", "2017-03-20", "DescribeDBInstances", "Items"),
        "redis" => ("redis", "2018-04-12", "DescribeInstances", "InstanceSet"),
        "mongodb" => ("mongodb", "2019-07-25", "DescribeDBInstances", "InstanceDetails"),
        _ => return Err(format!("不支持的数据库类型: {}", db_type)),
    };
    let mut out = Vec::new();
    // mongodb 的 Limit 上限为 100，统一按 100/页 翻页拉全量
    let (page_size, max_pages) = (100, 20);
    for page in 0..max_pages {
        let payload = json!({ "Limit": page_size, "Offset": page * page_size });
        let resp = ch.call_service(service, version, action, &payload).await?;
        let mut got = 0;
        if let Some(items) = resp[list_key].as_array() {
            for it in items {
                got += 1;
                let id = it["InstanceId"].as_str().unwrap_or("").to_string();
                if id.is_empty() {
                    continue;
                }
                let name = it["InstanceName"].as_str().unwrap_or("").to_string();
                out.push(DbInstance { id, name });
            }
        }
        if got < page_size {
            break;
        }
    }
    Ok(out)
}

// ---------- 指标目录 ----------

#[derive(Clone, Copy, PartialEq)]
pub enum Stat {
    Max,
    Avg,
}

pub struct CatalogEntry {
    pub key: &'static str,
    pub ns: &'static str,
    pub metric: &'static str,
    pub stat: Stat,
}

pub fn catalog(db_type: &str) -> &'static [CatalogEntry] {
    const CDB: &str = "QCE/CDB";
    const RDS: &str = "QCE/REDIS_MEM";
    const MGO: &str = "QCE/CMONGO";
    match db_type {
        "mysql" => &[
            CatalogEntry { key: "cpu", ns: CDB, metric: "CpuUseRate", stat: Stat::Max },
            CatalogEntry { key: "mem", ns: CDB, metric: "MemoryUseRate", stat: Stat::Max },
            CatalogEntry { key: "qps", ns: CDB, metric: "QPS", stat: Stat::Max },
            CatalogEntry { key: "tps", ns: CDB, metric: "TPS", stat: Stat::Max },
            CatalogEntry { key: "conn", ns: CDB, metric: "ThreadsConnected", stat: Stat::Max },
            CatalogEntry { key: "conn_limit", ns: CDB, metric: "MaxConnections", stat: Stat::Max },
            CatalogEntry { key: "conn_rate", ns: CDB, metric: "ConnectionUseRate", stat: Stat::Max },
            CatalogEntry { key: "threads_running", ns: CDB, metric: "ThreadsRunning", stat: Stat::Max },
            CatalogEntry { key: "slow", ns: CDB, metric: "SlowQueries", stat: Stat::Max },
            CatalogEntry { key: "innodb_hit", ns: CDB, metric: "InnodbCacheHitRate", stat: Stat::Avg },
            CatalogEntry { key: "commit", ns: CDB, metric: "ComCommit", stat: Stat::Max },
            CatalogEntry { key: "rollback", ns: CDB, metric: "ComRollback", stat: Stat::Max },
        ],
        "redis" => &[
            CatalogEntry { key: "cpu", ns: RDS, metric: "CpuUtil", stat: Stat::Max },
            CatalogEntry { key: "cpu_max", ns: RDS, metric: "CpuMaxUtil", stat: Stat::Max },
            CatalogEntry { key: "mem", ns: RDS, metric: "MemUtil", stat: Stat::Max },
            CatalogEntry { key: "mem_max", ns: RDS, metric: "MemMaxUtil", stat: Stat::Max },
            CatalogEntry { key: "conn_util", ns: RDS, metric: "ConnectionsUtil", stat: Stat::Max },
            CatalogEntry { key: "conn_max_util", ns: RDS, metric: "ConnectionsMaxUtil", stat: Stat::Max },
            CatalogEntry { key: "hit", ns: RDS, metric: "CmdHitsRatio", stat: Stat::Avg },
            CatalogEntry { key: "qps", ns: RDS, metric: "Commands", stat: Stat::Max },
            CatalogEntry { key: "conn", ns: RDS, metric: "Connections", stat: Stat::Max },
            CatalogEntry { key: "mem_used", ns: RDS, metric: "MemUsed", stat: Stat::Max },
            CatalogEntry { key: "keys", ns: RDS, metric: "Keys", stat: Stat::Max },
            CatalogEntry { key: "cmd_err", ns: RDS, metric: "CmdErr", stat: Stat::Max },
            CatalogEntry { key: "evicted", ns: RDS, metric: "Evicted", stat: Stat::Max },
            CatalogEntry { key: "expired", ns: RDS, metric: "Expired", stat: Stat::Max },
            CatalogEntry { key: "slow", ns: RDS, metric: "CmdSlow", stat: Stat::Max },
            CatalogEntry { key: "latency_avg", ns: RDS, metric: "LatencyAvg", stat: Stat::Max },
            CatalogEntry { key: "latency_max", ns: RDS, metric: "LatencyMax", stat: Stat::Max },
            CatalogEntry { key: "latency_p99", ns: RDS, metric: "LatencyP99", stat: Stat::Max },
            CatalogEntry { key: "flow_in", ns: RDS, metric: "InFlow", stat: Stat::Max },
            CatalogEntry { key: "flow_out", ns: RDS, metric: "OutFlow", stat: Stat::Max },
        ],
        "mongodb" => &[
            CatalogEntry { key: "cpu", ns: MGO, metric: "MonogdMaxCpuUsage", stat: Stat::Max },
            CatalogEntry { key: "cpu_avg", ns: MGO, metric: "MonogdAvgCpuUsage", stat: Stat::Avg },
            CatalogEntry { key: "mem", ns: MGO, metric: "MongodAvgMemUsage", stat: Stat::Avg },
            CatalogEntry { key: "mem_max", ns: MGO, metric: "MongodMaxMemUsage", stat: Stat::Max },
            CatalogEntry { key: "disk", ns: MGO, metric: "ClusterDiskusage", stat: Stat::Max },
            CatalogEntry { key: "mongos_cpu_avg", ns: MGO, metric: "MonogsAvgCpuUsage", stat: Stat::Avg },
            CatalogEntry { key: "mongos_cpu_max", ns: MGO, metric: "MonogsMaxCpuUsage", stat: Stat::Max },
        ],
        _ => &[],
    }
}

// ---------- 查询 ----------

async fn monitor_series(
    ch: &Channel,
    ns: &str,
    metric: &str,
    dims: &Value,
    start: i64,
    end: i64,
) -> Result<(Option<f64>, Option<f64>), String> {
    let payload = json!({
        "Version": "2018-07-24",
        "Namespace": ns,
        "MetricName": metric,
        "Instances": [{ "Dimensions": dims }],
        "Period": 60,
        "StartTime": iso_local(start),
        "EndTime": iso_local(end)
    });
    let resp = ch
        .call_service("monitor", "2018-07-24", "GetMonitorData", &payload)
        .await?;
    let mut max: Option<f64> = None;
    let mut sum = 0.0;
    let mut n = 0;
    if let Some(points) = resp["DataPoints"].as_array() {
        for p in points {
            if let Some(values) = p["Values"].as_array() {
                for v in values {
                    if let Some(f) = v.as_f64() {
                        max = Some(max.map_or(f, |m: f64| m.max(f)));
                        sum += f;
                        n += 1;
                    }
                }
            }
        }
    }
    let avg = if n > 0 { Some(sum / n as f64) } else { None };
    Ok((max, avg))
}

fn dims_for(db_type: &str, instance_id: &str) -> Value {
    match db_type {
        "redis" => json!([{ "Name": "instanceid", "Value": instance_id }]),
        "mongodb" => json!([{ "Name": "target", "Value": instance_id }]),
        _ => json!([{ "Name": "InstanceId", "Value": instance_id }]),
    }
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DbMetricValue {
    pub name: String,
    pub value: f64,
}

fn push(out: &mut Vec<DbMetricValue>, name: &str, v: Option<f64>) {
    if let Some(v) = v {
        out.push(DbMetricValue { name: name.to_string(), value: v });
    }
}

pub async fn query_db_metrics(
    ch: &Channel,
    db_type: &str,
    instance_id: &str,
    start: i64,
    end: i64,
    metrics: &[String],
) -> Result<Vec<DbMetricValue>, String> {
    let dims = dims_for(db_type, instance_id);
    let want = |key: &str| metrics.iter().any(|m| m == key);
    let mut out: Vec<DbMetricValue> = Vec::new();

    // 云监控指标
    for entry in catalog(db_type) {
        if !want(entry.key) {
            continue;
        }
        let (max, avg) = monitor_series(ch, entry.ns, entry.metric, &dims, start, end).await?;
        let v = match entry.stat {
            Stat::Max => max,
            Stat::Avg => avg,
        };
        push(&mut out, entry.key, v);
    }

    // DBbrain 特殊指标（磁盘 / 健康得分）
    match db_type {
        "mysql" if want("disk") => {
            let payload = json!({ "Version": "2019-10-16", "Product": "mysql", "InstanceId": instance_id });
            let resp = ch.call_service("dbbrain", "2019-10-16", "DescribeDBSpaceStatus", &payload).await?;
            let total = resp["Total"].as_f64();
            let remain = resp["Remain"].as_f64();
            let disk = match (total, remain) {
                (Some(t), Some(r)) if t > 0.0 => Some((t - r) / t * 100.0),
                _ => None,
            };
            push(&mut out, "disk", disk);
        }
        "redis" | "mongodb" if want("score") => {
            let payload = json!({
                "Version": "2019-10-16",
                "Product": db_type,
                "InstanceId": instance_id,
                "Time": iso_local(end)
            });
            let resp = ch.call_service("dbbrain", "2019-10-16", "DescribeHealthScore", &payload).await?;
            push(&mut out, "score", resp["Data"]["Value"].as_f64());
        }
        _ => {}
    }

    Ok(out)
}
