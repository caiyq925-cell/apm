// 数据库监控（MySQL / Redis / MongoDB）
// 指标来源：云监控 GetMonitorData（命名空间经实测/文档确认）+ DBbrain 空间接口（磁盘）
// 实测确认的口径：
//   MySQL   QCE/CDB     CpuUseRate/MemoryUse/QPS/ThreadsConnected/MaxConnections（维度 InstanceId）
//   Redis   QCE/REDIS_MEM CpuUtil/MemUtil/ConnectionsUtil/CmdHitsRatio（维度 instanceid，全小写）
//   MongoDB QCE/CMONGO   CpuUsage/MemUsage/DiskUsage（维度 InstanceId；若报维度错误需按控制台抓包校准）
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

/// 拉一条监控指标时间序列，返回 (最大值, 平均值)
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

pub async fn query_db_metrics(
    ch: &Channel,
    db_type: &str,
    instance_id: &str,
    start: i64,
    end: i64,
) -> Result<Vec<DbMetricValue>, String> {
    let dims = dims_for(db_type, instance_id);
    let mut out: Vec<DbMetricValue> = Vec::new();

    match db_type {
        "mysql" => {
            let (cpu, _) = monitor_series(ch, "QCE/CDB", "CpuUseRate", &dims, start, end).await?;
            let (mem, _) = monitor_series(ch, "QCE/CDB", "MemoryUseRate", &dims, start, end).await?;
            let (qps, _) = monitor_series(ch, "QCE/CDB", "QPS", &dims, start, end).await?;
            let (conn, _) = monitor_series(ch, "QCE/CDB", "ThreadsConnected", &dims, start, end).await?;
            let (conn_limit, _) = monitor_series(ch, "QCE/CDB", "MaxConnections", &dims, start, end).await?;
            // 磁盘使用率：DBbrain 空间概览 (Total-Remain)/Total
            let payload = json!({ "Version": "2019-10-16", "Product": "mysql", "InstanceId": instance_id });
            let resp = ch.call_service("dbbrain", "2019-10-16", "DescribeDBSpaceStatus", &payload).await?;
            let total = resp["Total"].as_f64();
            let remain = resp["Remain"].as_f64();
            let disk = match (total, remain) {
                (Some(t), Some(r)) if t > 0.0 => Some((t - r) / t * 100.0),
                _ => None,
            };
            push(&mut out, "cpu", cpu);
            push(&mut out, "mem", mem);
            push(&mut out, "qps", qps);
            push(&mut out, "conn", conn);
            push(&mut out, "conn_limit", conn_limit);
            push(&mut out, "disk", disk);
        }
        "redis" => {
            // 健康得分：DBbrain DescribeHealthScore（Time 必填）
            let payload = json!({ "Version": "2019-10-16", "Product": "redis", "InstanceId": instance_id, "Time": iso_local(end) });
            let resp = ch.call_service("dbbrain", "2019-10-16", "DescribeHealthScore", &payload).await?;
            let score = resp["Data"]["Value"].as_f64();
            push(&mut out, "score", score);

            let (cpu, _) = monitor_series(ch, "QCE/REDIS_MEM", "CpuUtil", &dims, start, end).await?;
            let (mem, _) = monitor_series(ch, "QCE/REDIS_MEM", "MemUtil", &dims, start, end).await?;
            let (conn_util, _) = monitor_series(ch, "QCE/REDIS_MEM", "ConnectionsUtil", &dims, start, end).await?;
            let (_, hit_avg) = monitor_series(ch, "QCE/REDIS_MEM", "CmdHitsRatio", &dims, start, end).await?;
            push(&mut out, "cpu", cpu);
            push(&mut out, "mem", mem);
            push(&mut out, "conn_util", conn_util);
            push(&mut out, "hit", hit_avg);
        }
        "mongodb" => {
            // 健康得分：DBbrain DescribeHealthScore（Time 必填）
            let payload = json!({ "Version": "2019-10-16", "Product": "mongodb", "InstanceId": instance_id, "Time": iso_local(end) });
            let resp = ch.call_service("dbbrain", "2019-10-16", "DescribeHealthScore", &payload).await?;
            let score = resp["Data"]["Value"].as_f64();
            push(&mut out, "score", score);

            // 实测：维度名为全小写 target；CPU/内存用 mongod 节点聚合指标，磁盘用整实例容量使用率
            let (cpu, _) = monitor_series(ch, "QCE/CMONGO", "MonogdMaxCpuUsage", &dims, start, end).await?;
            let (_, mem_avg) = monitor_series(ch, "QCE/CMONGO", "MongodAvgMemUsage", &dims, start, end).await?;
            let (disk, _) = monitor_series(ch, "QCE/CMONGO", "ClusterDiskusage", &dims, start, end).await?;
            push(&mut out, "cpu", cpu);
            push(&mut out, "mem", mem_avg);
            push(&mut out, "disk", disk);
        }
        _ => return Err(format!("不支持的数据库类型: {}", db_type)),
    }
    Ok(out)
}

fn push(out: &mut Vec<DbMetricValue>, name: &str, v: Option<f64>) {
    if let Some(v) = v {
        out.push(DbMetricValue { name: name.to_string(), value: v });
    }
}

#[allow(dead_code)]
fn unused(_: Value) {}
