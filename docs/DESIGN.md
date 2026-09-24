# 设计说明：会话模型、接口矩阵与指标口径

> 这份文档是取数设计的**唯一口径**，记录已经实测验证过的结论。加功能、排查问题先看这里。
> 接口结论都是用真实账号逐个试出来的；标 ✅ 表示当前可用。

---

## 一、工具形态

Windows 桌面小工具（Tauri v2 + 原生 HTML/JS，Rust 后端），把腾讯云上分散的监控数据
（APM 应用指标、MySQL/Redis/MongoDB、TKE 容器的 Pod 利用率）一次统计成一段 Markdown。

- 运行目录：`dist/`（绿色版，`apm-monitor.exe` + `WebView2Loader.dll` + `config.json`）
- 源码：`src-tauri/`（Rust）、`ui/`（前端三件套）
- 构建：`frontendDist = ../ui`，前端资源在编译时嵌进 exe —— **改了前端必须重新构建**，不能只替换 `ui/` 文件

---

## 二、会话模型：只有扫码登录

### 只有一种认证方式

早期支持过密钥模式（SecretId/SecretKey 签名调官方 OpenAPI）与手工粘贴凭证，**现已彻底移除**：
容器利用率等能力只有控制台链路有对应接口，密钥通道是死路。

界面上的唯一入口是设置里的**「扫码登录」**：弹出一个内嵌的腾讯云登录窗口，扫码（或登录态还在，
页面加载即可），程序经 CDP 自动取回会话并缓存。工具全程不接触账号密码。

### 会话只有三个字段

```jsonc
{
  "cookie": "……",                 // 控制台 Cookie 整串
  "csrfCode": "……",               // 控制台 CSRF 令牌
  "sessionFetchedAt": 1790130464   // 会话获取时间（Unix 秒）
}
```

`uin` / `ownerUin` **不是字段**，每次从 `cookie` 里解析（`Config::extract_ids_from_cookie`，
会去掉控制台加的前后缀 `o…` / `O…G`）。它们本来就是 Cookie 的派生品，存成字段只会出现
"字段与 Cookie 不一致"的事故。

### 会话由后端独有，前端不持有

- 前端拿到 `load_config` 的结果后会立刻删掉 `cookie / csrfCode / sessionFetchedAt`，拿不到也写不了。
- `save_config` 把前端传来的配置与磁盘上的合并：**用户配置用前端最新的，会话字段一律用磁盘上的**
  （`Config::merge_user_config`）。
- 所有需要会话的查询命令都先过 `with_session(config)`，同样把会话换成磁盘上的那份。

> 这条规则不是洁癖。改造前出过真实事故：前端手里一份过期配置，某刻把 `uin/ownerUin/csrfCode`
> 写成空值落盘，之后所有接口都报"无法确定 uin/ownerUin"。

### 会话维护参数（界面「设置 → 更多 → 会话维护」）

| 字段 | 界面文案 | 默认 | 含义 |
|---|---|---|---|
| `sessionRefreshMinutes` | 提前刷新阈值（分钟） | 10 | 会话年龄超过它就值得后台预刷新 |
| `sessionRefreshBackoffMinutes` | 刷新失败退避（分钟） | 2 | 一次刷新失败后先等这么久再试 |
| `sessionRefreshMaxFails` | 连续失败上限（次） | 3 | 连续失败到上限就停止后台刷新，等下次统计或手动登录 |
| `sessionAutoRefresh` | 后台自动刷新 | 开 | 总开关 |

---

## 三、失效判定与恢复

### 什么算"会话失效"（会触发重新登录）

判定集中在 `src-tauri/src/cred.rs`，规则是白名单：

- 本地就能判定：`会话不完整（Cookie 里解析不出 uin/ownerUin）`、`缺少 csrfCode`、`未登录`
- 服务端明确说不行：`code=9`（控制台 `/_api` 的 CSRF/登录态校验失败）、`HTTP 401/403`
- 中文文案兜底：`登录态过期`、`登录态验证失败`、`验证CSRF失败`、`请重新登录`
  （`/_api` 的错误里 `Code` 可能是 `"Unknown"`，只剩中文可判）

### 什么绝不算（不能拿去触发登录）

- **网络/传输类**：请求失败、超时、响应非 JSON、调试端口连不上 —— `cred::is_transport` 显式排除
- **参数/权限类**：`InvalidParameterValue`、`UnauthorizedOperation.CamNoAuth`、未选对象
- **旧网关 `/cgi/capi` 的令牌失效**（`code=1216`）—— 见下条，走 `cred::is_capi_token_stale`

误判代价很大：一次网络抖动会弹出登录窗口并让统计中断。

### 旧网关令牌失效 ≠ 会话失效（重要）

`/cgi/capi` 的 `code=1216 不合法的云 API 类型` 只说明**它自己那个短寿命令牌没被接受**：
实测换一份全新会话、甚至用控制台页面自己那份令牌，都可能照样 1216。所以：

- `cred::server_says_invalid` 的标记里**故意不含 `code=1216`**
- 这类错误走 `cred::is_capi_token_stale`，只用来决定"换个令牌重试一次"
- 效果：容器利用率失败时，只在容器那一行显示错误，**数据库/APM 的结果照常出来**

把两者混为一谈的后果是：容器一失败就把整次统计判成"需要重新登录"并中断，已经拿到的结果一起丢。

### 统计中遇到会话失效时发生什么

1. 后端把错误打上标记（`cred::cred` 在错误串前加不可见控制字符 `\u0001CRED\u0001`）
2. 前端在拿到任何数据源的结果后发现这个标记，立刻：
   - 调 `cancel_query` 真正中断后端还在跑的请求（不是干等）
   - 调 `notify_session_invalid`，让后端拉起登录窗口
   - 日志写 `统计中断：需要重新登录（已打开登录窗口，扫完请重新点「开始统计」），耗时 x.xs`
   - 状态栏写 `已中断：需要重新登录`，结果区保留上一次的结果不动
3. 登录窗口**显示但不抢焦点**（`focused(false)` + `alwaysOnTop(true)`），后台同时抓取会话；
   扫完由用户自己重新点「开始统计」

### 后台静默预刷新

`spawn_session_keeper`（`main.rs` 的 `setup` 里启动）每 120 秒醒一次：

- 关掉自动刷新、会话不可用、或有统计在跑（`QUERY_IN_FLIGHT`）→ 什么都不做
- 会话年龄没到阈值 → 什么都不做
- 连续失败已达上限 → 什么都不做
- 否则开一个**隐藏窗口**（`visible(false)`）去刷会话：成功不写日志（避免每十分钟刷屏），
  失败写 `后台刷新会话失败（第 n/m 次）：…` 并按退避参数多等一会儿

---

## 四、两条数据链路

控制台有**两条不同的链路**，令牌互不通用。

### 1. 控制台新网关 `console.rs` ✅

```
POST https://console-hc.cloud.tencent.com/_api/{service}/{Action}
     ?timeout=30000&t={毫秒}&uin=..&ownerUin=..&csrfCode=..
Body: { "cmd": Action, "serviceType": service, "data": {业务参数}, "regionId": 4 }
```

- 覆盖：APM 应用/指标、DBbrain、云监控指标（MySQL/Redis/MongoDB）、TKE 集群与命名空间列表、
  K8s 平台转发（负载/Pod 列表）
- 会话容忍度好（csrfCode 可放数小时），**绝大多数接口都走这里**
- `data` 缺 `Version` 时按 service 自动注入正确版本号

### 2. 控制台旧网关 `call_capi` ✅（仅容器利用率）

```
POST https://console.cloud.tencent.com/cgi/capi
     ?cmd={Action}&action=delegate&serviceType={service}
     &secure=1&version=3&dictId=2006&sts=1
     &t={毫秒}&uin=..&ownerUin=..&csrfCode=..
Body: { "text": "<内层 JSON 字符串>", "mime": "application/json", "encoding": "utf8" }
内层 JSON: { "cmd": Action, "serviceType": service, "data": {...}, "regionId": 4 }
```

**只有它能给出容器 CPU/内存利用率**（占 limit 的那两个指标）。APM 官方 API 与新的监控 API
对 `QCE/TKE2` 都返回空，这是实测结论。

**关键坑**：

- 请求 URL 里**不能带 `json=1`**。控制台自己发的请求没有它，加了会被判成另一种"云 API 类型"
- 它认的 `csrfCode` 和 `/_api` 那条**不是同一个东西**。用 `/_api` 那套打它，会得到
  `code=1216 不合法的云 API 类型`（这个文案极易误导，它说的不是参数错）
- 令牌寿命很短（实测：同一个令牌，浏览器当场用是 `code=0`，几十分钟后再用就 `code=1216`），
  这也是需要"十分钟预刷新"这套机制的原因
- **预热**：统计前先开一个隐藏的控制台工作负载页，让它自己发一次 dashboard 请求，
  我们从 CDP 里读出那次请求用的 `csrfCode`（`capi.rs`，缓存 5 分钟），
  **同时取回浏览器当前的 Cookie**。旧网关对 Cookie 的校验比 `/_api` 严：
  配置里那份是登录那一刻抓的，浏览器那份是实时的，所以容器这条链路优先用浏览器那份
  （`call_capi` 的 `cookie` 参数）。
- **请求载荷必须与控制台逐字对齐**。实测（`tools/capture_dump.json`）控制台的 dashboard 请求
  固定带 **5 个维度**（`region`/`tke_cluster_instance_id`/`pod_name`/`namespace`/`workload_name`）
  和 `Period`；少维度或少 `Period` 都可能导致取不到数据。载荷不对时**不一定是令牌问题**，
  别一看到失败就往 `code=1216` 上想。
- **脚本发的请求会被这个网关拒**。实测：令牌、载荷都对齐了，脚本（reqwest）直发仍回
  `code=1216 不合法的云 API 类型`，而**同一个页面自己 `fetch` 是 `code=0`** —— 差别在"谁发"
  （浏览器 Cookie / TLS / 来源）。所以失败时会退到 `capi.rs::fetch_dashboard()`：
  把 JS 塞进隐藏的控制台页面里执行，让浏览器自己发（见 HANDOFF 第二条"退路方案"）。

`Conditions[].Dimension` 是**数组里装一段 JSON 字符串**，元素形如
`{"Key":"pod_name","Value":["…"],"Operator":"in"}`，别改错形状。

---

## 五、接口 × 数据源矩阵（实测结论）

| 能力 | 通道 | 接口 / 参数要点 |
|---|---|---|
| APM 应用列表（含请求量） | 新网关 | `DescribeApmServiceMetric`（**不要带 Page/时间**，一次返回全部应用，实测 220 个） |
| APM 指标（应用/SQL/MQ/JVM） | 新网关 | `DescribeGeneralMetricData`：`ViewName` = `service_metric`/`sql_metric`/`mq_metric`/`runtime_metric`；`Filters` 必须含 `service.name`，**必须加 `{span.kind: server}`** 才是控制台口径；`Period=0` 取整段聚合 |
| APM 实例指标（CPU/内存最高点） | 新网关 | `DescribeMetricRecords`：**必须一次请求整套 12 个指标**（子集会报错），`GroupBy=["service.instance"]`，指标名 `cpu_usage_percent_top`/`jvm_heap_usage_percent_top`，多实例取最大 |
| APM 指标清单 | — | 官方无「指标清单」接口，工具内置实测清单 |
| TKE 集群 / 命名空间 | 新网关 | `tke.DescribeClusters`、`tke.DescribeClusterNamespaces`（`ClusterType` 参数不被识别，别传） |
| TKE 工作负载 / Pod（含 limit、env） | 新网关 | `tke.ForwardPlatformRequestV3`，`data = {Method:"GET", Path:"/apis/apps/v1/namespaces/{ns}/deployments", ClusterName:"cls-xxx"}`；Pod 用 `.../deployments/{name}/pods?limit=500`；返回体在 `ResponseBody`（JSON 字符串） |
| 单 Pod limit / `SW_AGENT_NAME` | 同上 | Pod/Deployment 模板里的 `resources.limits`（CPU 如 `2`/`500m`，内存如 `6Gi`）与容器 `env` 中的 `SW_AGENT_NAME`（**严格等于 APM 应用名**才关联） |
| 容器利用率（占 limit） | 旧网关 | `monitor.DescribeDashboardMetricData`，`Namespace=QCE/TKE2`，指标 `K8sPodRateCpuCoreUsedLimit`、`K8sPodRateMemNoCacheLimit`，`GroupBy=["InstanceId"]`，`QueryVersion=2020-10-21`，**必须带 `Period`**，维度串是**数组里装一段 JSON 字符串**，且必须与控制台一致地给全 **5 个维度**：`region`(eq) + `tke_cluster_instance_id`(in) + `pod_name`(in) + `namespace`(eq) + `workload_name`(eq)；响应 `Data[]` 每项带 `MetricName`，`Value` 是**字符串形式的 JSON 数组** |
| 数据库实例列表 | 新网关 | DBbrain `DescribeDiagDBInstances`（`Product` = `mysql`/`redis`/`mongodb`，`IsSupported=true`，每页 100 翻页）；**比产品接口全**：MySQL 95 vs CDB 接口 49 |
| MySQL 指标 | 新网关 | `GetMonitorData`，`QCE/CDB`，维度 `InstanceId`：`CpuUseRate`、`MemoryUseRate`、`QPS`、`TPS`、`ThreadsConnected`、`MaxConnections`、`ConnectionUseRate`、`ThreadsRunning`、`SlowQueries`、`InnodbCacheHitRate`、`ComCommit`、`ComRollback` |
| MySQL 磁盘使用率 | 新网关 | DBbrain `DescribeDBSpaceStatus`（`Total/Remain` → `(T-R)/T`） |
| Redis 指标 | 新网关 | `GetMonitorData`，**`QCE/REDIS_MEM`**（`QCE/REDIS` 已下线），维度小写 `instanceid`：`CpuUtil`、`CpuMaxUtil`、`MemUtil`、`MemMaxUtil`、`ConnectionsUtil`、`ConnectionsMaxUtil`、`CmdHitsRatio`、`Commands`、`Connections`、`MemUsed`、`Keys`、`CmdErr`、`Evicted`、`Expired`、`CmdSlow`、`LatencyAvg`、`LatencyMax`、`LatencyP99`、`InFlow`、`OutFlow` |
| MongoDB 指标 | 新网关 | `GetMonitorData`，`QCE/CMONGO`，维度 **`target`（小写）**：`MonogdMaxCpuUsage`、`MonogdAvgCpuUsage`、`MongodAvgMemUsage`、`MongodMaxMemUsage`、`ClusterDiskusage`（副本集无 Mongos，`Monogs*` 无数据→不输出该行） |
| 健康得分（Redis/Mongo） | 新网关 | DBbrain `DescribeHealthScore`，`Time` 必填（ISO 时间），取 `Data.Value` |

---

## 六、指标口径与格式化

- **APM**：默认只统计服务端跨度（`span.kind=server`），否则会把出站调用算进请求量（实测相差约 9 倍）。
- **容器利用率**：`使用量 / 单 Pod limit`，取窗口内所有 Pod 的最大值。
- **数值格式**：请求量 ≥1 万显示 `X.XW`；错误率保留两位小数；耗时 1 位小数 + ms；
  内存限制 `6Gi → 6GB`（<1GB 显示 MB）；计数类整数。
- **输出顺序**（容器块）：CPU 最大利用率 → 内存最大利用率 → APM 指标 → `Pod数` → `CPU` → `内存`。
- **默认勾选**：APM 默认只勾请求量、异常请求量、错误率、平均耗时、最大耗时
  （P95/P99/P50/实例 CPU·内存最高点/平均(次/秒) 需手动勾）；容器指标默认全勾。

---

## 七、可靠性设计

| 机制 | 说明 |
|---|---|
| 并发限流 | 查询任务用信号量限制并发（指标 8、数据库/容器 5-6），远低于腾讯云 20 QPS 限频 |
| 部分失败不阻塞 | 单个应用/实例/工作负载失败只在该结果块标注错误，其他照常输出 |
| 取消 | 前端「停止」→ `cancel_query` 置全局标志，后端在派发下一个任务前检查并中断；前端丢弃迟到结果 |
| 实时日志 | 后端 `app.emit("query-log", ...)` 推送进度，前端日志面板按时间戳展示（默认展开），出错标红。**依赖 `src-tauri/capabilities/default.json` 的 `core:default`**：Tauri v2 的 `plugin:event\|listen` 属插件命令，没有 capability 会被 ACL 拒绝；而 `listen` 返回 Promise，同步 `try/catch` 抓不到拒绝——一旦漏配，日志会**静默**变空 |
| 日志落盘 | 同一批消息同时写 `dist/apm-monitor.log`（`commands::log_to_file`，超 1MB 重建）。界面白屏时日志面板看不到，这是唯一的事后证据；启动时也写一条带 exe 路径的标记 |
| 渐进渲染 | 每个数据源返回即渲染一次（结果区显示"正在统计…（N/M 个数据源已返回）"），不等 `Promise.all` |
| 会话失效恢复 | 见第三节：中断 → 拉起不抢焦点的登录窗口 → 后台取回会话 → 用户重新点统计 |
| 后台预刷新 | 见第三节，空闲时隐藏窗口静默刷新 |
| 登录成功判定 | 必须同时满足：控制台 **API 请求**（`/_api/` 或 `/cgi/capi`）里出现过 `csrfCode=` + Cookie 里 `uin`/`ownerUin` 非空且写全；且**保存前先用新会话调一次 `list_clusters` 验证**，不通过就保留原会话 |
| 配置迁移 | 老 `config.json`（含 `authMode/secretId/secretKey/uin/ownerUin`）启动后自动清理这些字段并补齐新字段，用户配置无损 |

### 登录窗口的四个坑（Windows / WebView2）

1. **端口只能开在主窗口**：WebView2 环境按 data directory 共享，`additionalBrowserArgs` 只能在创建环境时生效，
   且同目录下各 webview 的该参数**必须一致**。所以端口统一写在 `tauri.conf.json` 主窗口，
   `login::BROWSER_ARGS` 必须与它**逐字一致**；登录窗口、预热窗口都用这个常量。
   漏传或传了不同的值，WebView2 就建不出第二个 webview，表现是"日志说窗口已打开、界面上却没有窗口"。
2. **窗口 label 要带唯一后缀**（`cloud-login-<毫秒>` / `cloud-capi-<秒>`）：`build()` 失败后 label 会被永久占用，
   之后一直报 `a webview with label cloud-login already exists`；`close()` 是异步的，紧接着建同名窗口也会撞。
   创建前按前缀清理，并用 `LOGIN_IN_FLIGHT` 保证同一时刻只有一个登录/刷新流程。
3. **连上 CDP 后不要再 `Page.navigate`**：窗口本来就用目标 URL 建的，再导航一次会打断控制台页面初始化，
   抓到的是"半初始化" Cookie（缺 `saas_synced_session`/`web_uid`），服务端一律判 `code=9`。
   未登录时控制台自己会跳登录页、扫码后自动跳回，不需要额外导航。
4. **Cookie 扁平化要跳过空值、同名优先控制台域**：同名 Cookie 可能分布在 `.tencent.com` 与
   `.cloud.tencent.com` 两个域上，扁平化成一个 Cookie 头时会被后来的空值覆盖（实测 `uin=` 空值把
   `uin=o765378326` 顶掉）。`build_cookie` 已按此处理，`cookie_has(key)` 也要求"非空值"。
   另外 `ownerUin=0` 是控制台 URL 里的占位值，真值在 Cookie 里（`ownerUin=O100012781415G`），
   不能直接采用。

---

## 八、已知限制 / 后续可做

1. **容器利用率只有旧网关 dashboard 接口能提供**：令牌几分钟过期，需预热与自动刷新窗口；
   载荷必须与控制台一致（5 个维度 + `Period`）。若集群接入 Prometheus(TMP) 可改走
   Prometheus（token 长期有效、无弹窗）。当前状态见 `docs/HANDOFF-会话与登录改造.md` 第二条。
2. **扫码抓取偶发拿到"半套 Cookie"**：校验会拦住、原会话不会被写坏，但用户会看到一次失败。
   已把 csrfCode 来源收窄到 API 请求，仍待实测确认。
3. **APM 指标数量有限**：官方无指标清单接口，已把实测可用的 20 项全部内置
   （应用 8 + 计算 1 + 实例 2 + SQL 3 + MQ 3 + JVM 3）。
4. **Pod 级 CPU/内存的"历史窗口峰值"**依赖 dashboard 接口；`metrics.k8s.io` 无法通过平台转发访问（已实测）。
5. 未做：多账号切换、告警推送、图表趋势（目前是区间聚合值）。

---

## 九、构建与发布

```bash
# 1) 停掉正在运行的程序（否则 exe 被占用，复制会失败）
# 2) 构建
cd src-tauri && cargo build --release
# 3) 更新本地绿色版目录（exe 和 DLL 一起）
cp src-tauri/target/release/apm-monitor.exe dist/
cp src-tauri/target/release/WebView2Loader.dll dist/
# 4) 重新打开 dist/apm-monitor.exe（桌面快捷方式指向这里）
```

> ⚠️ **构建后请确认界面不是白屏**：`python tools/smoke_gui.py`（读 `location.href` +
> `document.scripts.length` + 截窗口图）。**白屏的真因是 WebView2 用户数据目录被强杀弄坏**
> （`%LOCALAPPDATA%\com.local.apmmonitor\EBWebView`），删掉该目录重启即可，
> **不是包的问题**。详见 `docs/HANDOFF-会话与登录改造.md` 坑九。
> 调试时**不要用 `taskkill /F`**，正常关闭或发 `WM_CLOSE`。

**注意**：`dist/config.json` 是使用者的真实配置，升级时**不要覆盖**。打绿色版 zip 时也不要把
`config.json` 打进去（含会话 Cookie/csrfCode）。
发布到 GitHub Release 只认 `v*` 标签（工作流在 `.github/workflows/`）。
