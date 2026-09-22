# 设计说明：凭证模型、接口矩阵与指标口径

> 这份文档记录**已经实测验证**的取数设计，后续加功能/排查问题时先看这里。
> 所有接口都用真实账号逐个试出来的，标注 `✅已实测` 表示当前可用。

---

## 一、两种凭证，各司其职

工具支持两种凭证，**同一个功能可以按需选择通道**：

| 凭证 | 载体 | 有效期 | 适用 |
|---|---|---|---|
| **密钥（SecretId/SecretKey）** | TC3-HMAC-SHA256 签名，官方 OpenAPI | 长期（月/年） | 指标类数据（APM/容器利用率/数据库指标） |
| **Cookie（控制台会话）** | `Cookie` 头 + URL 上的 `uin/ownerUin/csrfCode` | 数小时（其中旧网关令牌仅几分钟） | 控制台私有能力：K8s 资源列表、平台转发、部分 dashboard 指标 |

**混合原则（重要）**：资源列表（Deployment/Pod/`SW_AGENT_NAME`/limit）官方 API 没有对应接口，只能用 Cookie；指标优先用密钥（长期有效）。因此推荐两者都填。

**凭证来源（三种，任选）**
1. **内置扫码登录**：应用打开一个登录窗口加载腾讯云官方页面，登录后经 CDP（`Network.getAllCookies` + 监听 `Network.requestWillBeSent` 取 `csrfCode`）把会话写回 `config.json`。窗口使用应用私有浏览器配置目录，登录态可复用。
2. **Reqable 抓包提取**（备选）：拉起 Reqable 官方 `mcp-server.exe`，按其 MCP 协议查询抓包记录，取最近一条含 `csrfCode` 的控制台请求，解析出 `Cookie/uin/ownerUin/csrfCode`。
3. **粘贴 cURL**：解析浏览器「复制为 cURL」的内容（兼容 cmd `^` 转义与 bash 引号），提取上述四项。

---

## 二、三个调用通道

### 1. 官方 API（密钥模式）`tc3.rs`
- 域名：`{service}.tencentcloudapi.com`，`Version` 走 `X-TC-Version` 头，`Region` 走 `X-TC-Region` 头。
- **发送前会剔除控制台专用字段**（`Version`/`Language`/`Region`/`regionId`/`SpaceUUID`/`Module`），否则官方返回 `UnknownParameter`。

| service | Version | 用到的 Action |
|---|---|---|
| apm | 2021-06-22 | `DescribeApmServiceMetric`、`DescribeGeneralMetricData`、`DescribeMetricRecords` |
| monitor | 2018-07-24 | `GetMonitorData`（数据库指标）、`DescribeBaseMetrics` |
| cdb / redis / mongodb | 2017-03-20 / 2018-04-12 / 2019-07-25 | 实例列表（兜底） |
| dbbrain | 2019-10-16 | `DescribeHealthScore`、`DescribeDBSpaceStatus`、`DescribeDiagDBInstances` |
| tke | 2018-05-25 | `DescribeClusters`、`DescribeClusterNamespaces` |

### 2. 控制台新网关（Cookie 模式）`console.rs` ✅
```
POST https://console-hc.cloud.tencent.com/_api/{service}/{Action}
     ?timeout=30000&t={毫秒}&uin=..&ownerUin=..&csrfCode=..
Body: { "cmd": Action, "serviceType": service, "data": {业务参数}, "regionId": 4 }
```
- 会话容忍度好（csrfCode 可放数小时），**绝大多数接口都走这里**。
- 若 `data` 缺少 `Version`，会自动按 service 注入正确版本号。

### 3. 控制台旧网关（Cookie 模式）`call_capi` ✅
```
POST https://console.cloud.tencent.com/cgi/capi
     ?cmd={Action}&action=delegate&serviceType={service}
     &secure=1&version=3&json=1&dictId=2006&sts=1
     &t={毫秒}&uin=..&ownerUin=..&csrfCode=..
Body: { "text": "<内层 JSON 字符串>", "mime": "application/json", "encoding": "utf8" }
内层 JSON: { "cmd": Action, "serviceType": service, "data": {...}, "regionId": 4 }
响应: { code: 0, data: { code: 0, data: { Response: {...} } } }
```
- **仅用于容器 CPU/内存利用率**（`monitor.DescribeDashboardMetricData`）。
- 会话令牌只有几分钟有效，过期返回 `code=1216`；工具会在此时**自动打开登录窗口刷新会话并重试一次**。
- 注意 `dictId` 取值与服务有关：TKE/监控用 `2006`，APM 用 `2211`。

---

## 三、接口 × 数据源矩阵（实测结论）

| 能力 | 通道 | 接口 / 参数要点 |
|---|---|---|
| APM 应用列表（含请求量） | 新网关／官方 | `DescribeApmServiceMetric`（**不要带 Page/时间**，一次返回全部应用，实测 220 个） |
| APM 指标（应用/SQL/MQ/JVM） | 新网关／官方 | `DescribeGeneralMetricData`：`ViewName` = `service_metric`/`sql_metric`/`mq_metric`/`runtime_metric`；`Filters` 必须含 `service.name`，**必须加 `{span.kind: server}`** 才是控制台口径；`Period=0` 取整段聚合 |
| APM 实例指标（CPU/内存最高点） | 新网关／官方 | `DescribeMetricRecords`：**必须一次请求整套 12 个指标**（子集会报错），`GroupBy=["service.instance"]`，指标名 `cpu_usage_percent_top`/`jvm_heap_usage_percent_top`，多实例取最大 |
| APM 指标清单 | — | 官方无「指标清单」接口，工具内置实测清单 |
| TKE 集群 / 命名空间 | 新网关／官方 | `tke.DescribeClusters`、`tke.DescribeClusterNamespaces`（`ClusterType` 参数不被识别，别传） |
| TKE 工作负载 / Pod（含 limit、env） | 新网关（Cookie） | `tke.ForwardPlatformRequestV3`，`data = {Method:"GET", Path:"/apis/apps/v1/namespaces/{ns}/deployments", ClusterName:"cls-xxx"}`；Pod 用 `.../deployments/{name}/pods?limit=500`；返回体在 `ResponseBody`（JSON 字符串） |
| 单 Pod limit / `SW_AGENT_NAME` | 同上 | Pod/Deployment 模板里的 `resources.limits`（CPU 如 `2`/`500m`，内存如 `6Gi`）与容器 `env` 中的 `SW_AGENT_NAME`（**严格等于 APM 应用名**才关联） |
| 容器利用率（占 limit） | 旧网关（Cookie） | `monitor.DescribeDashboardMetricData`，`Namespace=QCE/TKE2`，指标 `K8sPodRateCpuCoreUsedLimit`、`K8sPodRateMemNoCacheLimit`，维度 `region + tke_cluster_instance_id + pod_name`（JSON 字符串形式），`Value` 为 JSON 数组字符串 |
| 容器指标（官方 API） | 官方 | ⚠️ 实测对 `QCE/TKE2` **无数据**（多种维度组合/周期均为空），故不作为主通道；新网关 `GetMonitorData` 同样为空 |
| 数据库实例列表 | 新网关／官方 | DBbrain `DescribeDiagDBInstances`（`Product` = `mysql`/`redis`/`mongodb`，`IsSupported=true`，每页 100 翻页）；**比产品接口全**：MySQL 95 vs CDB 接口 49 |
| MySQL 指标 | 新网关／官方 | `GetMonitorData`，`QCE/CDB`，维度 `InstanceId`：`CpuUseRate`、`MemoryUseRate`、`QPS`、`TPS`、`ThreadsConnected`、`MaxConnections`、`ConnectionUseRate`、`ThreadsRunning`、`SlowQueries`、`InnodbCacheHitRate`、`ComCommit`、`ComRollback` |
| MySQL 磁盘使用率 | 新网关／官方 | DBbrain `DescribeDBSpaceStatus`（`Total/Remain` → `(T-R)/T`） |
| Redis 指标 | 新网关／官方 | `GetMonitorData`，**`QCE/REDIS_MEM`**（`QCE/REDIS` 已下线），维度小写 `instanceid`：`CpuUtil`、`CpuMaxUtil`、`MemUtil`、`MemMaxUtil`、`ConnectionsUtil`、`ConnectionsMaxUtil`、`CmdHitsRatio`、`Commands`、`Connections`、`MemUsed`、`Keys`、`CmdErr`、`Evicted`、`Expired`、`CmdSlow`、`LatencyAvg`、`LatencyMax`、`LatencyP99`、`InFlow`、`OutFlow` |
| MongoDB 指标 | 新网关／官方 | `GetMonitorData`，`QCE/CMONGO`，维度 **`target`（小写）**：`MonogdMaxCpuUsage`、`MonogdAvgCpuUsage`、`MongodAvgMemUsage`、`MongodMaxMemUsage`、`ClusterDiskusage`（副本集无 Mongos，`Monogs*` 无数据→不输出该行） |
| 健康得分（Redis/Mongo） | 新网关／官方 | DBbrain `DescribeHealthScore`，`Time` 必填（ISO 时间），取 `Data.Value` |

---

## 四、指标口径与格式化

- **APM**：默认只统计服务端跨度（`span.kind=server`），否则会把出站调用算进请求量（实测相差约 9 倍）。
- **容器利用率**：`使用量 / 单 Pod limit`，取窗口内所有 Pod 的最大值。
- **数值格式**：请求量 ≥1 万显示 `X.XW`；错误率保留两位小数；耗时 1 位小数 + ms；内存限制 `6Gi → 6GB`（<1GB 显示 MB）；计数类整数。
- **输出顺序**（容器块）：CPU 最大利用率 → 内存最大利用率 → APM 指标 → `Pod数` → `CPU` → `内存`。
- **默认勾选**：APM 默认只勾请求量、异常请求量、错误率、平均耗时、最大耗时（P95/P99/P50/实例 CPU·内存最高点/平均(次/秒) 需手动勾）；容器指标默认全勾。

---

## 五、可靠性设计

| 机制 | 说明 |
|---|---|
| 并发限流 | 查询任务用信号量限制并发（指标 8、数据库/容器 5-6），远低于腾讯云 20 QPS 限频 |
| 部分失败不阻塞 | 单个应用/实例失败只在该结果块标注错误，其他照常输出 |
| 取消 | 前端「停止」→ `cancel_query` 置全局标志，后端在派发下一个任务前检查并中断；前端丢弃迟到结果 |
| 实时日志 | 后端 `app.emit("query-log", ...)` 推送进度，前端日志面板按时间戳展示，出错标红 |
| 会话自动刷新 | 旧网关 `code=1216` → 自动打开登录窗口取新会话 → 重试一次（等待上限 120s） |
| 配置迁移 | 启动时自动补齐新增指标项、迁移数据源/场景结构、清理已下线字段，老 `config.json` 可直接用 |
| 错误提示 | 错误信息带上定位提示（如 1216 附带"刷新会话"指引） |

---

## 六、已知限制 / 后续可做

1. **容器利用率只有控制台 dashboard 接口能提供**：会话几分钟过期，需自动刷新窗口；若集群接入 Prometheus(TMP) 可改走 Prometheus（token 长期有效、无弹窗）。
2. **密钥权限**：密钥需有对应产品权限；TKE 官方接口若报 `CamNoAuth`，当前实现已改为资源列表走 Cookie，不影响使用。
3. **APM 指标数量有限**：官方无指标清单接口，已把实测可用的 20 项全部内置（应用 8 + 计算 1 + 实例 2 + SQL 3 + MQ 3 + JVM 3）。
4. **Pod 级 CPU/内存的"历史窗口峰值"**依赖 dashboard 接口；`metrics.k8s.io` 无法通过平台转发访问（已实测）。
5. 未做：多账号切换、告警推送、图表趋势（目前是区间聚合值）。
