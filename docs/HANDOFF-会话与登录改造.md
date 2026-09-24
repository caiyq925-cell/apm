# APM 监控工具 · 会话与登录改造交接文档

本文档面向接手这段改造的开发者。目标是：**看完这一份就能独立验证已完成的改动、并把剩下的工作做完**，不需要任何对话上下文。

---

## 一、这个工具是什么

一个 Windows 桌面小工具（Tauri v2 + 原生 HTML/JS，Rust 后端），用来把腾讯云上一堆分散的监控数据（APM 应用指标、MySQL/Redis/MongoDB、TKE 容器的 Pod 利用率）**一次性统计成一段 Markdown**，方便粘贴到汇报里。

它不调用官方 OpenAPI（早期支持过密钥模式，现已彻底移除），而是**复用腾讯云控制台的私有接口**：用你自己的登录态（Cookie）去调控制台页面在用的那些接口。

- 运行目录：`dist/`（绿色版，`apm-monitor.exe` + `WebView2Loader.dll` + `config.json`）
- 源码：`src-tauri/`（Rust）、`ui/`（前端三件套）
- 构建：`src-tauri/tauri.conf.json` 里 `frontendDist = ../ui`，前端资源在编译时嵌进 exe

---

## 二、改造后的凭证与会话模型（目标状态）

### 只有一种认证方式：扫码登录

界面上不再有密钥模式、不再有"粘贴 cURL""从 Reqable 抓取""手动填 uin/ownerUin/csrfCode/Cookie"这些入口。**唯一的入口是设置里的「扫码登录」按钮**：弹出一个内嵌的腾讯云登录窗口，你扫码（或登录态本来就还在，页面加载即可），程序自动取回会话并缓存。

### 会话只有三个字段

`config.json` 里与会话有关的字段只剩：

```jsonc
{
  "cookie": "……",            // 控制台 Cookie 整串
  "csrfCode": "……",         // 控制台 CSRF 令牌
  "sessionFetchedAt": 1790130464   // 会话获取时间（Unix 秒）
}
```

`uin` / `ownerUin` **不再是字段**，每次要用的时候从 `cookie` 里解析（`Config::extract_ids_from_cookie`，会把控制台加的 `o…` / `O…G` 前后缀去掉）。这样做是因为这两个值本来就是 Cookie 的派生品，存成字段只会出现"字段和 Cookie 不一致"的事故。

### 会话由后端独有，前端不持有

- 前端调用 `load_config` 拿到配置后会立刻 `delete cfg.cookie / cfg.csrfCode / cfg.sessionFetchedAt`，它拿不到也写不了会话。
- `save_config` 会把前端传来的配置与磁盘上的配置合并：**用户配置用前端最新的，会话字段一律用磁盘上的**（`Config::merge_user_config`）。
- 所有需要会话的查询命令（应用列表、数据库、容器、集群/命名空间/负载列表）都先过一层 `with_session(config)`，同样是把会话换成磁盘上的那份。

> 这条规则不是洁癖：改造前就发生过一次真实事故——前端手里一份过期配置，某个时刻把 `uin/ownerUin/csrfCode` 三个字段写成空值落盘，导致之后所有接口都报"无法确定 uin/ownerUin"。

### 会话维护参数（界面上可调）

放在「设置 → 更多 → 会话维护」，写在 `config.json`：

| 字段 | 界面文案 | 默认 | 含义 |
|---|---|---|---|
| `sessionRefreshMinutes` | 提前刷新阈值（分钟） | 10 | 会话年龄超过它就值得后台预刷新 |
| `sessionRefreshBackoffMinutes` | 刷新失败退避（分钟） | 2 | 一次刷新失败后先等这么久再试 |
| `sessionRefreshMaxFails` | 连续失败上限（次） | 3 | 连续失败到这个次数就停止后台刷新，等下次统计或手动登录 |
| `sessionAutoRefresh` | 后台自动刷新 | 开 | 总开关 |

---

## 三、失效判定与恢复流程

### 什么算"会话失效"（会触发重新登录）

判定集中在 `src-tauri/src/cred.rs`，规则是白名单：

- 本地就能判定的：`会话不完整（Cookie 里解析不出 uin/ownerUin）`、`缺少 csrfCode`、`未登录`
- 服务端明确说会话不行：`code=9`（控制台 `/_api` 的 CSRF/登录态校验失败）、`code=1216`（旧网关令牌失效）、`HTTP 401/403`
- 控制台那几条中文文案兜底：`登录态过期`、`登录态验证失败`、`验证CSRF失败`、`请重新登录`（因为 `/_api` 的错误里 `Code` 可能是 `"Unknown"`，只剩中文可判）

### 什么绝不算（不能拿去触发登录）

- **网络/传输类**：请求失败、超时、响应非 JSON、调试端口连不上 —— `cred::is_transport` 显式排除
- **参数/权限类**：`InvalidParameterValue`、`UnauthorizedOperation.CamNoAuth`、未选对象

误判的代价很大：一次网络抖动会弹出登录窗口并让统计中断。

### 统计中遇到会话失效时发生什么

1. 后端把错误标上标记（`cred::cred` 会在错误串前加一个不可见控制字符 `\u0001CRED\u0001`）
2. 前端在拿到任何数据源的结果后发现这个标记，立刻：
   - 调 `cancel_query` 真正中断后端还在跑的请求（不是干等）
   - 调 `notify_session_invalid`，让后端拉起登录窗口
   - 日志写一条 `统计中断：需要重新登录（已打开登录窗口，扫完请重新点「开始统计」），耗时 x.xs`
   - 状态栏写 `已中断：需要重新登录`，结果区保留上一次的结果不动
3. 登录窗口是**显示但不抢焦点**（`focused(false)` + `alwaysOnTop(true)`），后台同时抓取会话；扫完由用户自己重新点「开始统计」

### 后台静默预刷新

`spawn_session_keeper`（在 `main.rs` 的 `setup` 里启动）每 120 秒醒一次：

- 关掉自动刷新、会话不可用、或有统计在跑（`QUERY_IN_FLIGHT`）→ 什么都不做
- 会话年龄没到阈值 → 什么都不做
- 连续失败已达上限 → 什么都不做
- 否则开一个**隐藏窗口**（`visible(false)`）去刷会话：成功不写日志（避免每十分钟刷屏），失败写一条 `后台刷新会话失败（第 n/m 次）：…` 并按退避参数多等一会儿

---

## 四、两条数据链路（改代码前必须理解）

工具用的是控制台的**两条不同的链路**，它们的令牌互不通用：

### 1. `console-hc.cloud.tencent.com/_api/{service}/{Action}`（新网关）

- 覆盖：APM 应用/指标、DBbrain、云监控指标（MySQL/Redis/MongoDB）、TKE 集群与命名空间列表、K8s 平台转发（负载/Pod 列表）
- 参数：URL 带 `uin / ownerUin / csrfCode`，body 是 `{cmd, serviceType, data, regionId}`
- 会话来源：扫码登录抓到的 `cookie` + `csrfCode`

### 2. `console.cloud.tencent.com/cgi/capi?...&cmd=…&action=delegate&serviceType=monitor&…&sts=1`（旧网关）

- **只有它能给出容器 CPU/内存利用率**（占 limit 的那两个指标）。APM 官方 API 和新的监控 API 对 `QCE/TKE2` 都返回空，这是实测结论
- 参数：URL 带 `uin / ownerUin / csrfCode`，body 是**内层 JSON 直发**

> ⚠️ **2026-09-24 更正：1216 的真根因（推翻此前所有排查）**
> 旧网关的请求体是**内层 JSON 直发**（`{"cmd":..,"serviceType":..,"data":{..},"regionId":4}`），
> 抓包 content-length 逐条核过，71 条请求全部吻合。`mime`/`encoding` 是 Reqable 的
> 元数据字段、不是协议的一部分。包一层 `{"text":"..."}` 的话网关解析不到顶层 `cmd` →
> 无法识别云 API 类型 → `code=1216 不合法的云 API 类型`。
> 此前所有"令牌短命/TLS/会话/csrf/x-lid/x-life"的排查都是在这个错误前提上造的假问题——
> **脚本直发（配置会话 + 直发 body）实测 `code=0` 且有数据，根本不需要浏览器/预热窗口/那些头。**

- **关键坑**：
  - 请求 URL 里**不能带 `json=1`**。控制台自己发的请求没有它，加了会被判成另一种"云 API 类型"
  - **请求体 = 内层 JSON 直发，绝不能包 `{"text":"..."}`**（见上方更正；这是 1216 的真根因）
  - `csrfCode` 就是 `bkn(skey)`（腾讯经典哈希 `h=5381; h+=33*h+ord(ch)` 取低31位），
    `/_api` 和 `/cgi/capi` 是**同一个**；会话有效期内它是确定的，会话轮换后跟着变 →
    发请求时用 `Config::capi_csrf()` 现算
  - 容器链路**不需要浏览器/预热窗口**：`apm::call_capi` 用配置会话直发旧网关
    （见 `container.rs::pod_util_metrics`）；`capi.rs` 只保留"页面自己发"的兜底
    推论：① 两条链路的 csrf 是**同一个**（旧记忆"互不通用"是错的）；② csrf 不是独立签发的
    短寿命令牌，它随 `skey` 存亡——真正过期的是**会话**（`skey` 被控制台定时刷新轮换），
    csrf 只是它的哈希，所以"csrf 十分钟失效"的体感其实是"会话十分钟被轮换"。
    配置里只要 `skey` 有效，`bkn(skey)` 就是对的 csrf。

控制台自己的请求长这样（从真实抓包里核过，抓包文件见 `tools/capture_dump.json`，是用户用 Reqable 抓的容器监控页）：

```
POST https://console.cloud.tencent.com/cgi/capi
     ?cmd=DescribeDashboardMetricData&action=delegate&serviceType=monitor
     &secure=1&version=3&dictId=2006&sts=1&t=<ms>&uin=<uin>&ownerUin=<ownerUin>&csrfCode=<token>
Body: {"cmd":"DescribeDashboardMetricData","serviceType":"monitor","regionId":4,
       "data":{"Version":"2018-07-24","Language":"zh-CN","SpaceUUID":"space_default",
                "Module":"monitor","Query":[{"Datasource":"DS_QCEMetric","Namespace":"QCE/TKE2",
                "MetricName":"K8sPodRateCpuCoreUsedLimit",
                "Conditions":[{"Region":"ap-shanghai","Dimension":["{…一段 JSON 字符串…}"]}],
                "GroupBy":["InstanceId"],"Period":60,"StartTime":"…","EndTime":"…",
                "QueryVersion":"2020-10-21"}]}}
```

注意 `Conditions[].Dimension` 是**数组里装一段 JSON 字符串**，元素形如 `{"Key":"pod_name","Value":["…"],"Operator":"in"}`。代码里的 `dimension_json` 已经是这个形状，别改错。**Body 就是这一层，没有别的包装。**

---

## 五、必须守住的不变量

1. **查询命令的会话一定来自磁盘**（`with_session`），不要直接用前端传来的 `config`。
2. **保存前先校验**：`validate_login_session` 用新会话真调一次 `list_clusters`，不过就不写盘、保留原会话。不要为了"快"跳过它。
3. **旧网关的错误不要标成"会话失效"**（详见"还没做的工作"第一条），否则容器令牌一失效会把整次统计连别的数据源一起中断。
4. **主窗口和登录窗口、预热窗口的 `additionalBrowserArgs` 必须逐字一致**（见坑第二条）。
5. **窗口 label 必须唯一**（见坑第三条）。
6. **连上 CDP 后不要 `Page.navigate`**（见坑第四条）。
7. 前端不持有会话字段；会话字段与用户配置分开处理。

### 这些不变量现在有测试兜着

`cd src-tauri && cargo test` —— **离线可跑，不需要账号**（共 28 个测试）。改动下面这些逻辑前先跑一遍：

| 不变量 | 测试 |
|---|---|
| 旧网关 1216 不算会话失效 | `cred::tests::capi_1216_is_not_session_invalid`、`capi_preheat_timeout_is_token_stale_only` |
| 网络抖动绝不触发登录 | `cred::tests::transport_never_triggers_login` |
| 参数/权限类不触发登录 | `cred::tests::parameter_and_permission_errors_are_not_cred` |
| `/_api` 的会话失效仍能识别 | `cred::tests::api_session_failure_is_session_invalid` |
| 会话字段以磁盘为准（防前端写坏） | `config::tests::merge_user_config_keeps_disk_session_but_takes_user_fields` |
| 空会话不覆盖已有会话 | `config::tests::apply_login_does_not_overwrite_with_empty_values` |
| `uin`/`ownerUin` 从 Cookie 派生并剥前后缀 | `config::tests::extracts_ids_from_cookie_stripping_console_affixes` |
| 容器载荷带全 5 个维度 | `container::tests::dimension_json_has_all_five_dims_the_console_sends` |
| 1 小时窗口的 `Period` = 60 | `container::tests::period_matches_console_for_short_windows` |
| 利用率响应解析（`Value` 是 JSON 数组字符串） | `container::tests::parse_util_data_takes_max_across_points` |
| Cookie 扁平化不制造"半套" | `login::tests::build_cookie_skips_empty_and_prefers_console_domain` |
| 诊断日志不泄露 Cookie 值 | `login::tests::describe_cookie_never_leaks_values` |

---

## 六、踩过的坑（现象 → 原因 → 结论）

### 坑一：前端监听后端事件永远收不到（日志面板只有前端自己写的行）

- 现象：后端 `log_to()` 发的进度日志一条都没到前端；`try { listen(...) } catch {}` 看起来也没报错。
- 原因：Tauri v2 里 `plugin:event|listen` 是**插件命令**，要 capability 授权。这个应用当时**没有任何 capability**（`src-tauri` 下无 `capabilities/` 目录），命令被 ACL 拒绝。而 `listen()` 返回的是 Promise，**同步 `try/catch` 抓不到 Promise 的拒绝**，于是静默失效。
- 结论：新增 `src-tauri/capabilities/default.json`（`windows: ["main"]` + `permissions: ["core:default"]`），并把注册写成 `.catch(...)`。**以后加任何插件调用/事件监听，先确认 capability 覆盖到了。**

### 坑二：登录窗口"日志说已打开、界面上却没有窗口"

- 原因：Windows 上 WebView2 的环境是**按 data directory 共享**的（`tauri-runtime-wry` 里 `web_context_key = data_directory`）。而 `additionalBrowserArgs` 只在创建环境时生效；同一 data directory 下的多个 webview，该参数**必须一致**，不一致就建不出第二个 webview（`tauri-utils` 的 `WindowConfig::additional_browser_args` 文档原话：不同的值必须配不同 data directory）。
- 结论：远程调试端口统一写在 `tauri.conf.json` 主窗口的 `additionalBrowserArgs`，代码里的 `login::BROWSER_ARGS` 与它**逐字相同**；登录窗口、预热窗口都用这个常量。

### 坑三：`a webview with label cloud-login already exists`

- 原因：`build()` 失败后 label 会被永久占用；`close()` 是异步的，紧接着 `build()` 同名窗口也会报这个错。
- 结论：label 带唯一后缀（`cloud-login-<毫秒>` / `cloud-capi-<秒>`），创建前按前缀清理，并用 `LOGIN_IN_FLIGHT` 保证同一时刻只有一个登录/刷新流程。

### 坑四：抓回来的会话服务端不认（`code=9 登录态验证失败`）

- 原因：连上 CDP 后立刻 `Page.navigate` 会打断控制台页面的初始化，抓到的是一套"半初始化"的 Cookie（缺 `saas_synced_session` / `web_uid` 等）。
- 结论：窗口本来就是用目标 URL 建的，**不要再导航**；未登录时控制台自己会跳登录页，扫码后自动跳回来。

### 坑五：Cookie 扁平化时会丢信息

- 现象：抓回来的 Cookie 里 `uin` 是空的，保存后所有接口报错。
- 原因：`getAllCookies` 拿到的同名 Cookie 可能分布在 `.tencent.com` 和 `.cloud.tencent.com` 两个域上，扁平化成一个 Cookie 头时会被后来的空值覆盖。
- 结论：`build_cookie` 已改成**跳过空值、同名优先取控制台域（`cloud.tencent.com`）**。`cookie_has(key)` 也改成要求"非空值"，否则 `uin=`（空）会被误判成"有 uin"。

### 坑六：旧网关的 `1216 不合法的云 API 类型` 不是参数错

- 实测：把 URL 里的 `json=1` 去掉也照样 1216；请求体逐字段和控制台一致也照样 1216。用浏览器会话原样回放到脚本里也 1216，而浏览器自己发是 `code=0`。
- 结论：这是**令牌没被接受**的表现（令牌与会话绑定、寿命约十分钟），报错文案是误导。不要照字面去调参数。

### 坑七：`ownerUin` 抓成 `0`

- 原因：控制台的请求 URL 里常见 `ownerUin=0` 占位，真值在 Cookie 里（`ownerUin=O100012781415G`）。
- 结论：`login.rs` 的 `extract_id` 把 `0` 视为"没提供"，交给 Cookie 兜底。

### 坑八：终端里跑 PowerShell 脚本报"引号未终止"

- 原因：Windows PowerShell 5.1 按 ANSI 读 `.ps1`，中文注释会把引号弄坏。
- 结论：`tools/` 下的 `.ps1` 一律**纯 ASCII**，需要中文名字时用 `[char[]](0x…)` 拼。

### 坑九（2026-09-23 踩到）：界面全白 —— WebView2 浏览器进程崩溃，不是包的问题

- **现象**：exe 能启动、窗口也在（标题「APM 监控」，尺寸正常），但**界面一片空白**。
  没有任何报错，日志面板也是空的（因为前端根本没跑起来）。
- **根因（有崩溃转储为证）**：**WebView2 的浏览器进程在启动时崩溃**。
  证据：应用数据目录的 Crashpad 报告里出现了新鲜的 `.dmp`：
  ```
  %LOCALAPPDATA%\com.local.apmmonitor\EBWebView\Crashpad\reports\*.dmp
  ```
  （本次排查时一次找到 3 个，时间戳与白屏那几次启动一一对应。）
  崩溃 → webview 没能挂上 → 窗口空白；同时调试端口也会在 4~6 秒后消失。
- **对照证据**（同一个 exe、同一份配置，只是启动时机不同）：
  | 状态 | 结果 |
  |---|---|
  | 崩溃 | 白屏：`about:blank`、`scripts=0`、空 body；端口 4~6 秒后消失 |
  | 正常 | `location.href=http://tauri.localhost/`、`scripts=2`、`body≈8200`；端口稳定存活 30 秒+ |
  → **同一个二进制两种结果，所以不是应用代码的问题。**
- **恢复**：删掉数据目录重启即可（会自动重建），已验证两次有效：
  ```bash
  rm -rf "$LOCALAPPDATA/com.local.apmmonitor/EBWebView"
  # 或者：python tools/diagnose_blank_ui.py   （清掉 + 启动验证两次）
  ```
  代价是登录窗口里缓存的登录态没了（`config.json` 里的会话不受影响）。
- **⚠️ 触发条件仍未查明**。已知**强杀（`taskkill /F`、任务管理器"结束任务"）会触发**；
  但也遇到"两次启动之间没跑过应用、目录却是坏的"。遇到白屏**直接按上面恢复**，别先找原因。
- **环境侧的可疑因素（已做对照实验排除应用侧参数）**：
  本机装了**两个 WebView2 运行时版本**
  （`C:\Program Files (x86)\Microsoft\EdgeWebView\Application\153.0.4234.32` 与 `…\.48`）。
  崩溃元数据（`Crashpad/watson_metadata`，结构化文本）指向：
  ```
  ProcessType=browser        ← 崩的是 WebView2 的浏览器进程
  ModuleName=msedge.dll      ModuleVersion=153.0.4234.48
  SubCode=0x80000003         ← STATUS_BREAKPOINT = Chromium 的 CHECK 断言失败
  ```
  **对照实验（每档 6 次启动，用截图非白像素占比判断是否渲染）**：
  | 浏览器参数 | 渲染 | 新增崩溃转储 |
  |---|---|---|
  | 现有（`--disable-features=… --remote-debugging-port=9223`） | 3/6 | 4 |
  | 去掉 `--remote-debugging-port` | 3/6 | 4 |
  | 完全不带参数 | 5/6 | 4 |
  → **参数不是触发原因**（差异在 6 次样本下不显著），且崩溃转储在三种配置下都产生。
  另外 `Crashpad/temp/edge_shutdown_crash.txt` 的存在说明**部分崩溃发生在关闭阶段**，
  所以"崩溃转储数"不能直接等同于"启动失败次数"。
  **结论：这是机器层面的 WebView2 不稳定，应用侧改不掉。**
  建议修复/重装 WebView2 运行时（跑 Microsoft 的 Evergreen 安装器），必要时清理掉旧的
  `153.0.4234.32` 目录。应用侧唯一的"非默认"浏览器参数是 `--remote-debugging-port=9223`，
  但它不能去掉（登录/预热都依赖 CDP），而且实验证明去掉它也没用。
- **怎么判断是不是这个坑**：`python tools/smoke_gui.py`（启动 + 读 `location.href`/`scripts`/`body` + 截窗口图），
  以及看 `dist/apm-monitor.log` 里有没有启动标记（有标记说明程序起来了，是 webview 崩了而不是没启动）。
- **⚠️ 给排查的人**：调试时**不要强杀进程**，要么界面上正常关闭，要么给主窗口发 `WM_CLOSE`
  （`tools/smoke_gui.py` 就是这么收尾的）。否则你可能自己制造出这个 bug，再去怀疑前端、
  capability、构建方式——本次就是这么被绕进去的。**先用 `smoke_gui.py` 看一眼 `location.href`，能省几小时。**

> 附：`Cargo.toml` 里后来加的 `default = ["custom-protocol"]` 是**另一件事**（让 `cargo build --release`
> 与 `npx tauri build` 等价，符合 Tauri 官方脚手架惯例），**它不是白屏的原因**。
> 曾经误判成白屏根因并写进本节，已更正。

---

## 六之二、日志落盘（2026-09-23 第三轮）

**位置**：`dist/apm-monitor.log`（exe 同目录，便携版约定；目录不可写时退到 `%TEMP%\apm-monitor.log`）。
**内容**：界面日志面板里的**全部**消息，外加一条启动标记：

```
=== 应用启动 v0.1.1 | exe: D:\...\dist\apm-monitor.exe
```

**两条来源都要接**（这点踩过坑）：
- 后端产生的行 → `commands::log_to`（emit + 落盘）；
- **前端自己产生的行**（`开始统计…` / `统计完成：N 个对象…` / `统计中断：需要重新登录…` 等）
  → 前端 `logLine` 会调 `log_ui` 命令落盘。**不接这一半的话，文件里就没有"统计完成"，
  排查时很容易把一次正常结束误判成"卡住了"。**
- 后端事件推给前端的那些行（`query-log`）由 `logLine(msg, kind, false)` 跳过落盘，避免重复。
- ⚠️ 在 `capi.rs` 这类地方**不要直接用 `app.emit`**，要走 `log_to_file` + emit ——
  否则那行只进界面、不进文件（`容器指标令牌预热成功（…）` 就这么丢过一次）。

**为什么要有它**：界面日志面板有两个"看不到"的场景 ——
① **WebView2 数据目录坏了导致界面全白**（坑九），面板压根渲染不出来；
② 查询中断或关闭程序后想事后复盘。
有文件日志就能直接把文件发过来，不用靠截图和口述。

**排查时先看这几行**（容器利用率问题）：

| 日志行 | 含义 |
|---|---|
| `容器统计范围：集群 … / 命名空间 …，N 个工作负载，M 项 APM 指标` | 本次统计的范围（前端已打过「开始统计…」，这里不重复） |
| `容器指标令牌预热成功（令牌长度 N，浏览器 Cookie M 字符，x-lid 已取，x-life 基准已取…）` | 预热拿到了令牌与 x-lid/x-life 基准；任一项"缺失"都说明旧网关头没抓全，容器会被网关拒 |
| `容器指标令牌预热失败：…` | 隐藏窗口没发出 dashboard 请求；若提示"页面被送到了登录页"则**需要重新扫码** |
| `<工作负载>: 容器利用率取到 2 项 —— cpu_util_limit=…, mem_util_limit=…` | 利用率取到了（成功路径） |
| `<工作负载>: 容器指标查询失败: 旧网关接口错误 code=1216…` | 脚本直发被拒（会触发退路） |
| `…；退路也失败：旧网关接口错误 code=1216…（在页面里发也一样…）` | 连页面自己发都不行，不是"谁发"的问题 |
| `<工作负载>: APM[<view>] 取到 N 项 —— request_count=…, duration_avg=…` | **APM 指标取到了什么**（第四轮补上，以前只有"正在查询"没有结果） |
| `<实例>（<名字>）: 取到 N 项 —— health_score=…, cpu_usage=…` | **数据库指标取到了什么**（第四轮补上，以前一条都不打） |
| `数据库指标查询完成：成功 X / 共 N 个实例` | 数据库那批的汇总 |

超过 1MB 会自动删掉重建，不会无限增长。`*.log` 已在 `.gitignore` 里。

---

## 七、已经做完、需要验证的清单

以下改动**都已经写进代码并编译发布**（`dist/apm-monitor.exe`，构建时间 2026-09-23 20:28）。标"已实测"的是我跑过的，其余请按步骤验一遍。

> 绿色版包 `dist/apm-monitor.zip` 已重打（只含 exe + DLL）。**旧包里含一份 9/22 的 `config.json`（带会话凭证），已不再放入新包**——分享包时不会再连带泄露会话。

### 验证速查

**离线可验的部分**（不需要账号，先跑这个）：`cd src-tauri && cargo test` —— 28 个测试全绿即代码侧不变量成立。
下面这张表是需要真人操作的部分。

`tools/` 里的脚本**都不需要 pip 安装**，在仓库根目录直接跑。

| # | 项目 | 怎么跑 | 状态 |
|---|---|---|---|
| 0 | **包能不能用（界面渲染）** | `python tools/smoke_gui.py` —— 必须是 `http://tauri.localhost/` + scripts>0 + body>0 | 已实测通过 |
| 0b | 日志落盘 | 启动后看 `dist/apm-monitor.log` 是否有 `=== 应用启动 v0.1.1 \| exe: …` | 已实测通过 |
| 1 | 配置结构改造与老配置迁移 | 放一份老 `config.json` 进 `dist/` → 启动 → 随便改个设置触发保存 → 看文件 | 已实测通过 |
| 2 | 界面精简与会话状态行 | 打开设置看 Cookie 区 + 「更多 → 会话维护」四项默认值 | 已实测通过 |
| 3 | 会话维护四项的持久化 | 改值/取消勾选 → 关程序 → 重开 → 看界面与文件 | 未验证 |
| 4 | 扫码取回会话（含保存前校验） | 点「扫码登录」；失败时看日志新增的 `抓到的 Cookie 完整度：…` | 部分实测 |
| 5 | 统计中会话失效 → 中断 + 拉窗 + 自动取回 | 备份 `dist/config.json` → 把 `csrfCode` 改成垃圾值 → 点「开始统计」 | 未用此法验证过 |
| 6 | 后台静默预刷新 | 阈值改 1 分钟 → `sessionFetchedAt` 改成当前减 600 → 启动 → 等 2~3 分钟看时间戳 | 未验证 |
| 7 | 正常统计的完整结果 | 点「开始统计」，看日志末行 `统计完成：N 个对象，耗时 x.xs` | 部分实测 |
| 8 | 容器 CPU/内存利用率 | 打开应用并**先扫码一次**，然后 `python tools/verify_container.py`（自动点统计 + 摘日志给结论）；或手动点「开始统计」后看 `dist/apm-monitor.log` | 未通过（载荷/令牌/Cookie/退路都已修，待验） |

排查辅助（按需）：

```bash
python tools/time_container.py     # 容器链路：K8s 列表 + 旧网关 code/Data，最快定位
python tools/time_probe.py         # 数据库/APM 链路耗时与 code
python tools/capi_in_browser.py    # 令牌口径对照实验（需要登录窗口开着）
python tools/cdp_cookie.py         # 抓登录窗口 Cookie + 试探会话可用性
python tools/findwin.py <PID>      # 拿主窗口 HWND
powershell -File tools/startquery.ps1 -Hwnd <HWND>   # 自动点「开始统计」
powershell -File tools/openlogin.ps1 -Hwnd <HWND>    # 自动展开设置并点「扫码登录」
powershell -File tools/uia_expand.ps1 -Hwnd <HWND>   # 展开日志面板并 dump 无障碍树
```


### 1. 配置结构改造与老配置迁移

- 怎么验：拿一份改造前的老 `config.json`（里面还有 `authMode/secretId/secretKey/uin/ownerUin`）放到 `dist/`，启动程序，随便改一个设置触发保存，然后看文件。
- 判定：`authMode/secretId/secretKey/uin/ownerUin` 消失；出现 `sessionFetchedAt/sessionRefreshMinutes/sessionRefreshBackoffMinutes/sessionRefreshMaxFails/sessionAutoRefresh`；`cookie/csrfCode` 保持原值；场景、勾选、指标等用户配置无损。
- 状态：**已实测通过**。

### 2. 界面精简与会话状态行

- 怎么验：打开设置，看 Cookie 区。
- 判定：只有「扫码登录」按钮 + 一行状态（形如 `已登录 · uin 765378326 · 会话获取于 09-23 10:41（3 分钟前）`）；「更多 → 会话维护」四项在，且有默认值 10 / 2 / 3 / 勾选。
- 状态：**已实测通过**（渲染正确）。

### 3. 会话维护四项的持久化

- 怎么验：把阈值改成别的数字/取消勾选 → 关闭程序 → 重新打开 → 看界面与文件。
- 判定：界面上还是改后的值，文件里对应字段也变了。
- 状态：**未验证**（代码路径是 `scheduleSave` → `save_config` → 合并落盘，逻辑上直接）。

### 4. 扫码取回会话（含保存前校验、失败不覆盖）

- 怎么验：点「扫码登录」，等到日志出现结论；再故意造一次失败（例如在抓取过程中把 `dist/config.json` 的 `csrfCode` 改成垃圾值），看点「扫码登录」后是否保留原会话。
- 判定：成功时日志 `会话已更新（cookie N 字符，uin=… ownerUin=…）`；失败时日志 `新会话未通过校验（…），已保留原会话`，且 `config.json` 里的会话字段没有被写坏。
- 状态：**部分实测**——成功路径与失败拦截都出现过（失败那次是抓取偶发拿到半套 Cookie，属正常拦截）。**已知瑕疵**：偶发会抓到半套 Cookie 导致这一次登录报失败（详见"还没做"第三条）。

### 5. 统计中会话失效 → 中断 + 拉起窗口 + 自动取回

- 怎么验（推荐用可控方式造失效，不动真实账号）：
  1. 备份 `dist/config.json`
  2. 把其中的 `csrfCode` 改成一个垃圾值（例如 `123`）
  3. 点「开始统计」
  4. 观察日志与界面
- 判定：日志出现 `统计中断：需要重新登录（已打开登录窗口，扫完请重新点「开始统计」），耗时 x.xs`；状态栏变成 `已中断：需要重新登录`；结果区保持上一次结果；随后出现一个**不抢焦点**的登录窗口，并（因为本机登录态还在）自动取回会话，日志 `会话已更新[…]`；再点一次统计能正常出结果。
- 状态：**已实测到中断 + 开窗 + 自动取回**（当时是被容器链路触发的中断），**用上面这套"改坏 csrfCode"的方式还没验过**，建议照着跑一遍。

### 6. 后台静默预刷新

- 怎么验：
  1. 「更多 → 会话维护」把阈值临时改成 1 分钟，确保勾选自动刷新
  2. 关闭程序，把 `config.json` 里的 `sessionFetchedAt` 改成当前时间减 600（即"会话已经 10 分钟没刷新了"）
  3. 启动程序，等 2~3 分钟，看 `sessionFetchedAt` 是否被自动更新
- 判定：`sessionFetchedAt` 变成新的时间戳；期间**不应**出现可见窗口（隐藏刷新），成功也不该写日志；把 `csrfCode` 改坏再试，则应出现 `后台刷新会话失败（第 1/3 次）：…` 并在退避时间后才重试。
- 状态：**未验证**。

### 7. 正常统计的完整结果（数据库 / 容器 / APM）

- 怎么验：点「开始统计」，等完成。
- 判定：日志最后是 `统计完成：N 个对象，耗时 x.xs`；MongoDB/Redis 各项有值（健康得分、CPU、内存、磁盘等）；容器那条的 APM 指标（请求量、耗时、错误率）有值。
- 状态：**部分实测**——数据库与容器 APM 指标都出过值，但**容器 CPU/内存利用率那两行仍是 `-`**（见下条）。

### 8. 容器 CPU/内存利用率（旧网关）

- 现状：**真机已复现**（2026-09-23），报 `容器指标查询失败: 旧网关接口错误 code=1216: 不合法的云 API 类型`。
  预热是成功的（否则错误文案会是预热自己的错），载荷也已对齐控制台 —— 所以是**网关不接受脚本发起的请求**，
  即坑六描述的现象。详见"还没做"第二条。
- 已修的两处（都不再是当前失败原因，但都是必要的正确性修复）：
  1. 误判成会话失效 → 现在容器这一步失败**不会再中断整次统计**（已由用户实测确认）；
  2. 请求载荷与控制台不一致（少 `namespace`/`workload_name` 与 `Period`）→ 已对齐。
- 怎么验：`python tools/capi_in_browser.py`（需登录窗口处于已登录状态），
  用"页面自己的令牌"和"配置里的令牌"各在页面里 `fetch` 一次，看是令牌问题还是"谁发"的问题。
- 判定：容器行出现百分比 → 成功；仍是 1216 且页面令牌也 1216 → 走"还没做"第二条的退路方案
  （让隐藏页面自己发、CDP 读响应）。

---

## 八、还没做的工作，以及具体怎么做

### 第一条（已落地）：不要让旧网关令牌失效把整次统计中断掉

**状态：已落地，且已完成收尾清理（2026-09-23 第二轮）。** 注意本小节原来写的"改法"是**照抄会把对的代码改回去**的，下面记录真实情况。

**原问题**：`apm.rs` 的 `call_capi` 把 `code=9 / 1216` 标成了"会话失效"。实测：换一份全新会话、
甚至用控制台页面自己那份令牌，这个网关都可能回 1216 —— 它不代表整机会话坏了。结果是容器利用率
一失败，前端就把整次统计中断，把已经拿到的数据库结果一起丢掉。

**当时的改法（已应用）**：`apm.rs::call_capi` 里 `code != 0` 时**不再**打 `cred` 标记，
只返回普通错误串：

```rust
if code != 0 {
    let msg = v["msg"].as_str().unwrap_or("");
    // 这里不标 cred：旧网关的 9/1216 只说明它自己的短寿命令牌没被接受，
    // 实测换一份全新会话也照样 1216，不代表整机会话失效——标了会把整次统计中断掉。
    return Err(format!(
        "旧网关接口错误 code={}: {}（容器 CPU/内存利用率依赖该网关的短寿命令牌）",
        code, msg
    ));
}
```

> 文件顶部的 `use crate::cred;` **不能删**：`Channel::from_config` 与 `call_capi` 里
> "缺 uin/ownerUin""缺 csrfCode"两处仍要用它打标记（那才是真的会话问题）。

**第二轮补的收尾（本次）**：光去掉标记还不够，同一个 `code=1216` 在别处仍被当成会话失效——

| 位置 | 原状 | 改法 |
|---|---|---|
| `cred.rs::server_says_invalid` | 标记里有 `"code=1216"` | **删掉**。并加注释说明它属于旧网关令牌问题 |
| `cred.rs` | 无 | 新增 `is_capi_token_stale()`，只匹配 `旧网关接口错误` / `没能从控制台页面取到旧网关令牌` |
| `commands.rs` 容器重试判定 | `cred::is_session_invalid(&e)` | 改为 `cred::is_capi_token_stale(&e) \|\| cred::is_session_invalid(&e)`，注释说明两者语义不同 |
| `login.rs::read_capi_csrf` 预热超时 | 返回 `cred::cred(...)`（标了会话失效） | 改为普通错误串——预热页面没发请求不是会话失效 |

**为什么必须分开**：`is_session_invalid` 是"需要重新登录"的判据；`is_capi_token_stale` 是
"换个令牌重试一次"的判据。混在一起，容器利用率失败就会冒名顶替整机会话问题。

**验收**：造一次容器令牌失效（或干脆断网让容器那步失败），统计**不应该**中断，
数据库结果照常出来，只在容器那一行显示这个错误。

### 第二条：确认容器 CPU/内存利用率到底能不能拿到

**状态：真机已复现两次，是两个不同阶段的问题（都还没通过）。**

| 时间 | 现象 | 说明 |
|---|---|---|
| 早先 | `旧网关接口错误 code=1216: 不合法的云 API 类型` | 预热成功、载荷也已对齐，但**网关拒绝脚本发起的请求** → 转"退路方案" |
| 19:31（第四轮） | `没能从控制台页面取到旧网关令牌（…登录窗口里没有登录态…）` | **预热阶段就失败**：隐藏窗口的页面被送去登录页 → 见下面"预热失败的两个改进" |

> 第二次的原因是**浏览器会话没有登录态**（为修白屏清过 WebView2 数据目录，把登录窗口的登录态一起清掉了；
> `config.json` 里的会话还在，所以 APM/数据库正常）。**点一次「扫码登录」完成扫码**即可恢复这条路径，
> 之后才会走到 1216 那条判断上。

#### 真机结论（比任何推理都可靠）

用户实测（载荷修复后的包）：

```
容器指标查询失败: 旧网关接口错误 code=1216: 不合法的云 API 类型（容器 CPU/内存利用率依赖该网关的短寿命令牌）
```

两条推论：

1. **预热是成功的**。若预热失败，容器那行会是预热自己的错误文案
   （`没能从控制台页面取到旧网关令牌…`）而不是网关的 1216 —— 见 `commands.rs` 里
   `capi_csrf` 为 `Err` 时直接把它当结果的分支。所以"我们确实拿到了控制台页面自己那份令牌，
   拿去发请求却被拒"。
2. **这正是坑六描述的现象**（"用浏览器会话原样回放到脚本里也 1216，而浏览器自己发是 `code=0`"）。
   也就是说：**这个网关不接受"非浏览器发起"的请求**，与令牌值本身关系不大。

> **⚠️ 前置条件**：预热窗口靠**登录窗口所在的浏览器会话**去访问控制台页面。如果那个会话没有登录态
> （最典型的就是 WebView2 数据目录被清过，见坑九），控制台页面会被重定向到登录页，
> 于是**一个 `/cgi/capi` 请求都不会发出**，预热会失败并提示
> `…页面没有发出 dashboard 请求：可能是工作负载页打不开，也可能登录窗口里没有登录态 —— 点一次「扫码登录」即可`。
> 这种情况下先点一次「扫码登录」再测，否则测出来的不是 1216 那条路径。

> 顺带确认：容器这一步失败**没有中断整次统计**，数据库/APM 结果照常输出 —— 第一条的修复生效。

#### 下一步：先做一次对照实验，再动架构

在动手改架构之前，用 `tools/capi_in_browser.py` 区分两种可能（**需要先把登录窗口打开并处于已登录状态**）：

```bash
# 1) 打开应用 → 点「扫码登录」并完成扫码（让一个 console.cloud.tencent.com 的页面活着）
# 2) 另开终端
python tools/capi_in_browser.py
```

它会用**页面自己那份令牌**和**配置里那份令牌**各在页面上下文里 `fetch` 一次同样的 dashboard 请求：

- 页面令牌 → `code=0` 且有数据 → 说明"在页面里发"就行，**转下面的退路方案**（让页面自己发、我们读响应）；
  同时也说明问题不在令牌值，而在"请求由谁发出"（Cookie 新鲜度 / 请求来源 / TLS 指纹）。
- 页面令牌也 1216 → 连页面自己发都不行，那就不是"谁发"的问题，回到令牌/参数继续查。

#### 已经排除的（载荷）

翻 `tools/capture_dump.json`（你当初用 Reqable 抓的容器监控页）时发现：抓包里**恰好有**
应用需要的 `K8sPodRateCpuCoreUsedLimit` 和 `K8sPodRateMemNoCacheLimit` 两个指标的真实请求，
**响应是 `code=0`、带真实百分比**（例如 `[6.732, 5.299, 10.599, …]`）。

所以**接口对 `QCE/TKE2` 是通的**，问题不在"接口没有这个数据"。控制台请求与改前的应用请求差三处：

| 项 | 控制台（抓包） | 应用（改前） |
|---|---|---|
| 维度 | **5 个**：`region`、`tke_cluster_instance_id`、`pod_name`、**`namespace`**、**`workload_name`** | **3 个**，少了后两个 |
| `Period` | `60` | **没有这个字段** |
| `GroupBy` / `QueryVersion` / `Datasource` / `Namespace` | `["InstanceId"]` / `2020-10-21` / `DS_QCEMetric` / `QCE/TKE2` | 一致 ✓ |

**已改**（`container.rs` / `commands.rs`）：`dimension_json` 补上 `namespace`、`workload_name`；
`pod_util_metrics` 补上 `Period`（新增 `pick_period`，1 小时窗口就是控制台那个 `60`）；
签名加 `namespace` / `workload` 两个参数，`commands.rs` 两处调用点已同步。
**这两处已不是当前的失败原因**（改了之后仍是 1216），但它们是必要的正确性修复。

响应侧也核对过，解析逻辑没问题：`Data[]` 每条都带 `MetricName`（过滤用得上），
`Value` 是**字符串形式的 JSON 数组**（`container.rs` 的 `parse_util_data` 正是按这个写的，已有测试）。

#### 预热失败的两个改进（2026-09-23 第四轮，来自真机日志）

真机日志暴露了两件事，都不是"1216"那条路径，而是**预热阶段就失败了**：

```
[19:31:49] 共 3 个工作负载，开始逐个查询
[19:32:27] 容器指标令牌预热失败：没能从控制台页面取到旧网关令牌（…）
[19:33:03] iam-service: 容器指标查询失败: 没能从控制台页面取到旧网关令牌（…）
[19:33:03] 统计完成：4 个对象，耗时 74.5s
```

1. **干等 35 秒才失败**。未登录时控制台会把隐藏窗口的页面送去登录页，那种情况下一个
   `/cgi/capi` 请求都不会发出，于是白等满 35 秒。
   **改法**：在等待的**空闲间隙**用 `Runtime.evaluate` 读一次 `location.href`（只在没有事件时读，
   不会吃掉正在路上的 `Network.requestWillBeSent`），若判定是登录页（URL 含 login/sso/auth 且不含
   `/tke2/`）就**立刻**失败，并**标 `cred`** —— 那是真的"需要重新登录"，前端会据此把扫码窗口拉起来。
   错误文案也写清了按钮位置：`请在「设置」里点「扫码登录」完成扫码`。
2. **每个工作负载都重试一次预热**，3 个工作负载把耗时从 ~5s 拉到 74.5s。
   **改法**：`cred::is_capi_token_stale` 收窄为**只匹配 `旧网关接口错误`**（网关拒了我们的请求）。
   预热失败不再触发"换令牌重试"——换令牌对"页面没发出请求"没有任何帮助。
   这也让退路的触发条件更准：只有"网关拒了我们"才走退路。

> **用户反馈的另一半**：提示说"点一次「扫码登录」"，但界面上看不到这个按钮 ——
> 它在**折叠的「设置」面板里**。这是改造后的设计（唯一登录入口），但提示没写位置。
> 现在错误文案会写明「设置」；另外上面第 1 点让"未登录"这种情况会**自动弹出扫码窗口**，
> 用户不必自己找按钮。

#### 隐藏窗口必须串行（2026-09-23 第五轮，又一个真机日志挖出来的）

第二次真机测试（用户已扫码，预热通了）的日志：

```
[19:44:23] 共 3 个工作负载，开始逐个查询
[19:44:25] iam-service: 查询容器 CPU/内存利用率（15 个 Pod）…
[19:44:31] iam-service: 容器指标查询失败: 调试通道错误: WebSocket protocol error: Connection reset without closing handshake
```

**`Connection reset without closing handshake` 是我们自己造成的**：
`capi.rs` 的 `acquire()`（预热）和 `fetch_dashboard()`（退路）开头都会
**按 label 前缀清理"残留窗口"**，而它们跑在**并发的工作负载任务**里（信号量 5）。
三个工作负载同时重试预热 → 互相把对方的窗口关掉 → CDP 连接被掐断 → 谁也拿不到令牌。

更隐蔽的一点：`acquire` 的前缀 `cloud-capi` 会**连带匹配**退路窗口 `cloud-capi-fetch`，
所以两者不能各用一把锁。

**改法**：两处共用一个 `WINDOW_LOCK`，让"隐藏控制台窗口"这个资源真正串行。
正常路径（缓存命中、脚本直发成功）不受影响。

**同时补了两个日志缺陷**（都是这次真机日志暴露的）：

1. **`capi.rs` 里原来用 `app.emit` 直接推给前端面板，没走 `log_to`** —— 所以
   `容器指标令牌预热成功（…）` 这一行**只出现在界面上、不在 `dist/apm-monitor.log` 里**。
   排查时看到"日志里怎么没有预热成功这一行"就是这么来的。已改为统一走 `log_to_file` + emit。
2. **重试失败时只报重试那个错误**（`Err(e2)`），把"原来为什么失败"（比如 1216）从日志里抹掉了。
   已改成 `{}；换令牌重试也失败：{}`，两个错误都留着。

#### 已做的自动对照：改用**浏览器当前那份 Cookie**（2026-09-23 第三轮）

观察到一个关键细节：预热窗口**确实正常打开并加载了控制台 TKE 页面**（CDP 里能看到
`https://console.cloud.tencent.com/tke2/clu…` 的 page 目标），所以"预热没工作"被排除。
剩下的差异就是**请求由谁发出**。其中最可能、也最容易验证的一条是 **Cookie**：

- 配置里的 Cookie 是**登录那一刻**抓下来存进 `config.json` 的；
- 浏览器（WebView2 会话）里那份是**实时**的，可能已经被刷新过（`refreshSession` 等）；
- `/_api` 对这份差异不敏感（所以 APM/数据库都正常），但旧网关可能校验更严。

**改动**：`login.rs::read_capi_token()` 在读出 csrfCode 的同时，顺手用
`Network.getAllCookies` 把浏览器当前的 Cookie 也取回来（复用 `build_cookie` 的同一套扁平化规则）；
`capi.rs` 把两者一起缓存；`call_capi` 新增 `cookie: Option<&str>` 参数，
容器这条链路传浏览器那份，为空时退回配置里的。

**怎么判断这次改对了**：跑一次统计，看日志里那行

```
容器指标令牌预热成功（令牌长度 N，浏览器 Cookie M 字符，取自控制台页面自己的 capi 请求）
```

然后看容器那行：

- **出现百分比** → 就是 Cookie 新鲜度问题，收工；
- **仍是 1216** → Cookie 不是原因，转下面的退路方案（"谁发"的问题，多半是 TLS/来源校验）。

#### 退路方案（**已实现**，2026-09-23 第三轮）

核心思路：**不再自己组装 HTTP 请求，改成让隐藏的控制台页面自己 `fetch`**，
网络那一层（Cookie / TLS / 来源）完全交给浏览器。

- `capi.rs::fetch_dashboard()`：开一个隐藏窗口（`cloud-capi-fetch-*`）导航到工作负载详情页 →
  用 `login.rs::eval_in_page()` 往页面里塞一段 JS → JS 从
  `performance.getEntriesByType("resource")` 里取**页面自己那份 csrfCode**，
  用 `fetch(..., {credentials:"include"})` 发请求 → 把响应文本返回给 Rust → 剥出 `Data[]`。
- 触发时机：**只有**脚本直发被网关拒（`cred::is_capi_token_stale`，即 1216）时才走，
  网络类错误不走（走它也没用，白等）。脚本那条路成功时行为完全不变。
- 退路必须**串行**（`FETCH_LOCK`）：它按前缀清理残留窗口，多个工作负载并发会互相关掉对方的窗口。
- 若页面里发也报 1216，错误信息会明说"在页面里发也一样" —— 那就说明连浏览器自己发都不行，
  该回头查令牌/参数，而不是继续怀疑"谁发"。

**注意时序**：页面加载后才会发出自己的 `/cgi/capi` 请求（我们要从里面取 csrfCode），
所以 `fetch_dashboard` 里重试 8 轮、每轮隔 3 秒。若预热窗口里的页面打不开（比如工作负载页 404），
退路会以"页面还没发出 capi 请求"失败。

若这条路也不行，就诚实地把这一项标成"不可得"，界面上照旧显示 `-` 并在结果里写明原因。
不要为了让它"看起来有数据"去猜。

**验收**：容器 CPU/内存利用率两行要么出现真实的百分比，要么在结果里给出一句明确的、
不会让人误以为是会话问题的原因说明。

**旁证**：`python tools/time_container.py` 会直接把旧网关的 `code` 和 `Data` 条数打出来
（带 pod 过滤 / 不带 pod 过滤各一次），比跑整次统计更快定位。

#### 根因定论与修复（2026-09-23 晚，真机日志 + 抓包逐条核对）

**真机日志**（预热已成功、token/cookie 都是新鲜的）三连败：

```
容器指标查询失败: 旧网关接口错误 code=1216: 不合法的云 API 类型（…）；
  退路也失败：旧网关接口错误 code=-1: （在页面里发也一样，说明不是「谁发」的问题）；
  换令牌重试也失败：调试通道错误: WebSocket protocol error: Connection reset without closing handshake
```

**`code=-1` 是全新的错误码**——它说明"页面里 fetch 也失败"不是巧合，而是**我们发的请求
和控制台页面自己发的请求差了什么**。翻 `tools/capture_dump.json` 逐条核（27 条 `/cgi/capi`
请求 + 1 条 `/cgi/com` ping）：

- 全部带 **`x-lid`**（会话内恒定，本次为 `sJxn-Y7DfV`）与 **`x-life`**（**每个请求都不同**）。
- `x-life` 与请求时刻 `t` 严格满足 **`x-life = t_ms - 1790078075676`**（会话常数，27 条全部吻合）。
- 控制台请求还有 `accept: application/json, text/javascript, */*; q=0.01` 等，属浏览器默认，非关键。
- 我们的**脚本直发和页面 fetch 都不带 `x-lid`/`x-life`** → 分别被回 `1216` / `-1`。

**这就是"短寿命令牌"本体**：`x-lid`（会话级）+ `x-life`（每请求级、按时间递增）。此前
"补最新 x-lid/x-life 仍是 1216"的旧实验用的是**抓包里已经过期的旧值**，不能代表最新值不行。

**修复（已落地，`cargo test` 34 个全绿、`cargo build --release` 通过）**：

| 文件 | 改动 |
|---|---|
| `login.rs` | `CapiToken` 增加 `x_lid` / `life_epoch_ms`；`read_capi_token` 抓页面请求时顺带取这两个头（`life_epoch = t_ms - x_life`），头名大小写容错；**传输错误自动重连一次**（修 CDP reset 竞态的一半）；新增纯函数 `capi_xlife` / `capi_request_meta`（有测试） |
| `apm.rs` | `call_capi` 增加 `x_lid` / `life_epoch_ms` 参数，发请求时带 `x-lid` 头 + 现场生成的新鲜 `x-life` 头 |
| `container.rs` | `pod_util_metrics` 透传这两个参数 |
| `capi.rs` | `fetch_dashboard` 改走 `read_capi_token` 抓值（不再从 performance 记录现翻 csrf——那里只有 URL 拿不到头），JS 里带 `x-lid` + `x-life: String(Date.now() - life_epoch)`；预热/取数窗口销毁后**等 400ms 再开新窗**（刚销毁的目标还挂在 `/json/list`，连上即 WebSocket reset——上次"换令牌重试也失败"就是这么来的）；`code!=0` 且 `msg` 为空时**把原始响应附在错误串里**，便于再查 |
| `commands.rs` | 两处 `pod_util_metrics` 调用点同步新参数 |

#### 第二轮真机结论（2026-09-23 21:32）：x-lid/x-life 已对齐，但败在一个 JS 低级 bug

预热日志 `…x-lid 已取，x-life 基准已取` 说明抓取正常；脚本直发**仍 1216**（与"字节级回放也 1216"
的旧证据一致——**旧网关就是不接受非浏览器发起的请求**，头已对齐也没用，脚本直发是死路，
不必再调它）。但**页面内 fetch 的原始响应**（本次新加的诊断）揭了底：

```
原始响应: {"message":"请求包格式错误或大小超出限制"}
```

**根因**：`build_fetch_js` 里 `body: <对象字面量>`。`fetch` 收到普通对象**不会**序列化成 JSON，
而是 `String(obj)` 变成 `"[object Object]"`——网关收到这 16 个字节就回"请求包格式错误"。
这个 bug 从退路方案实现起就存在，此前所有"页面里发也失败"（`-1`）都是它，与令牌/头无关。

**修复**：`body: JSON.stringify(...)`（`capi.rs`，有测试钉住）。`tools/capi_in_browser.py`
顺手修了两处：请求体少了 `{text:...}` 双层包装、同样缺 stringify（它一直报 1216 的实验结论
有污染，以真机为准）。

#### 第三轮真机结论（2026-09-23 21:53）：x-lid/x-life + body 都对了，页面内 fetch 仍 1216

body 修好后，页面内 fetch 从 `-1`（格式错误）变成**真正的 1216**——说明请求已经走到了
网关的令牌校验环节，只是仍被拒。此时我们的请求与控制台自己发的请求**逐项已对齐**
（URL 参数、body、x-lid、新鲜 x-life、浏览器 Cookie、csrf=bkn(skey)），只剩 `Accept` 一个头
有差（fetch 默认 `*/*`，控制台 XHR 是 `application/json, text/javascript, */*; q=0.01`——
已补上）。

**悬而未决的关键问题**：预热窗口那个隐藏页面**自己发的** dashboard 请求，在 WebView2 会话里
到底回 `code=0` 还是 `1216`？
- 若 `code=0`：会话没问题，我们的复制还差最后一点 → 继续对齐（多半就是 Accept 之类）。
- 若 `1216`：页面自己发都被拒 → **这个 WebView2 会话驱动不了旧网关**（新鲜扫码会话缺了
  真实浏览器攒下的风控/会话 Cookie——对比发现 WebView2 只有 29 段、Edge 有 47 段，差
  `mfaRMId`/`web_uid`/`pgv_info` 等），再对齐也没用，得换思路（用真实浏览器会话）。

**已加诊断（本轮落地）**：预热时顺带把**页面自己发的那条 dashboard 请求的响应 code** 读出来，
预热日志变成 `…页面自发的 dashboard 请求 code=X…`。跑一次统计看这一行就能分晓。

> ⚠️ 教训：**不要用外部 CDP 连正在跑的应用**（交接文档早警告过，本次排查又踩实了——
> 我连了几次把 WebView2 调试端口弄降级，应用自己的预热/登录（都靠 CDP）会跟着挂）。
> 要诊断就改应用自己的代码（像这次的 page_code），重启应用，别从外部捅 CDP。
>
> ⚠️ 另一件事（顺带发现，未修）：**WebView2 浏览器会话不跨重启**——应用每次重启，浏览器
> 会话 Cookie（skey 等 session cookie 不落盘）就没了，容器链路要重新扫一次码。
> 配置里的会话（`config.json`）不受影响，APM/数据库照常。这是"每次重启都要重扫才能
> 查容器指标"的原因，值得单独优化（比如把容器链路的预热页 cookie 换成配置里那份）。

**怎么验**：跑一次统计，先看预热日志的 `页面自发的 dashboard 请求 code=`：
- `code=0` → 容器行应出现 `cpu_util_limit=…, mem_util_limit=…`（修复生效）；
- `code=1216` → 会话问题，转上面的换思路方向。

### 第三条：扫码抓取偶发拿到"半套 Cookie"

**现象**：点「扫码登录」偶尔会得到 `新会话未通过校验（…登录态验证失败…）`，也就是抓到的 Cookie 和 csrfCode 不是一套。校验拦住了、原会话没被写坏，但用户会看到一次失败。

**状态：代码侧已改（2026-09-23 第二轮），待实测确认。**

**已改**（`login.rs` / `commands.rs`）：

1. **csrfCode 只从 API 请求里取**。原来是从"任何 `console.cloud.tencent.com` 域名、URL 里带
   `csrfCode=` 的请求"里取第一个，很可能是 SSO 跳转那次请求的 token，而不是控制台 API 请求的 token。
   现在要求 URL 同时包含 `/_api/` 或 `/cgi/capi` 且包含 `csrfCode=`。
   （`uin`/`ownerUin` 仍从任意控制台请求里补全，这部分无害。）
2. **`cookie_is_complete` 收紧**：原来是 `ownerUin` **或** `refreshSession` 之一即可，
   现在要求 `uin` 非空 **且**（`ownerUin` 或 `refreshSession`）非空——三者缺一都说明还是半套。
3. **新增 Cookie 完整度日志**（`login.rs::describe_cookie`）：登录校验失败或没检测到登录态时，
   日志会多一行 `抓到的 Cookie 完整度：段数 N，uin=有/无，ownerUin=有/无，refreshSession=有/无，saas_synced_session=有/无`
   （**只打有/无，不打任何值**）。这样"偶发失败"就有了可对比的证据，不用再临时加日志。

**若仍复现，按这个思路继续查**：

1. 连续点十次「扫码登录」，把每次成功/失败连同那行 `Cookie 完整度` 记下来，对比差异——
   重点看失败那几次到底缺哪个 Cookie。若失败时缺的总是同一个，就去查它是哪一步才落盘的。
2. 看 `stable_since`（1.5 秒无变化）是否真的在"页面停止写 Cookie"之后才触发；
   必要时把 `settle_start` 的 25 秒 give-up 拉长。

**验收**：连续点十次「扫码登录」，十次都成功（或明确报"需要扫码"，而不是"校验失败"）。

### 第四条：更新设计文档

**状态：已完成（2026-09-23 第二轮）。** `docs/DESIGN.md` 已按本文档第二、三、四、五节重写，
成为唯一口径。旧文档里的"两种凭证 / 密钥模式 / `tc3.rs` / Reqable 抓包 / 粘贴 cURL"已全部删除
（这些能力代码里已不存在），旧网关示例 URL 里错误的 `json=1` 也已去掉。

### 第五条：跑完第七节那张验证清单

**状态：未做（需要真机 + 真实账号）。** 尤其是"统计中断"（用改坏 csrfCode 的方式）与
"后台静默预刷新"这两项，目前只有代码没有实测证据。

### 第六条：清理一次性脚本

**状态：已完成（2026-09-23 第二轮）。** 已删除 `tools/` 下 8 个一次性替换脚本
（`patch_autorefresh.py`、`patch_cancel_log.py`、`patch_capi.py`、`patch_combo.py`、
`patch_container.py`、`patch_logui.py`、`patch_noapm.py`、`patch_ns_search.py`）。
诊断脚本全部保留（见第十节）。注意：文档里写的 `_patch_*.py` 与实际的 `patch_*.py` 前缀不一致，
按实际名字删的。

### 第七条（遗留疑点，需真机证据）：`console.rs` 里还留着 `code=1216`

**现象**：`console.rs::cred_payload` 的白名单里仍有 `code == "1216"`，也就是把 `/_api`（新网关）
返回的 1216 当成"会话失效、要重新登录"。但本文档第三节明确说：**1216 是旧网关 `/cgi/capi`
的码**，`/_api` 的会话失效码是 `code=9`。同一份代码里对同一个码有两种解释，正是第一条要消掉的那类隐患。

**为什么没直接删**：查了 `tools/capture_dump.json`，里面只有 4 条 `/_api` 请求，且**都没抓到响应体**，
无法证明 `/_api` 到底会不会回 1216。按"不要为了看起来对就去猜"的原则，没有改行为。

**风险对比**（两边都轻微，所以不值得赌）：
- 留着：若 `/_api` 因非会话原因回 1216，会**多弹一次登录窗口**（正是第一条那类误判）。
- 删掉：若 `/_api` 真的用 1216 表示会话失效，会**少一次自动重新登录**，用户看到报错后手动点扫码即可。
  注意中文兜底（`登录态验证失败` / `验证CSRF失败` / `请重新登录`）仍在，真实会话失效大概率仍能识别。

**怎么定**：跑一次统计，若出现 `/_api` 报错，看它的 `code` 与 `Message` 是不是 1216；
或直接查 `console.rs` 的调用方日志。**有证据后再改，并把结论回填到本节。**

---

## 八之二、第二轮改动记录（2026-09-23）

本次改动**只动代码与文档，未做真机验证**（扫码要真人扫、统计要打真实账号）。改动清单：

| 文件 | 改动 |
|---|---|
| `src-tauri/src/cred.rs` | `server_says_invalid` 删掉 `"code=1216"` 标记；新增 `is_capi_token_stale()`；补模块注释说明两条判据的分工。**第四轮**：`is_capi_token_stale` 收窄为只匹配 `旧网关接口错误` —— 预热失败不再触发"换令牌重试"（那会让每个工作负载各白等 35 秒） |
| `src-tauri/src/commands.rs` | 容器利用率重试判定改为 `is_capi_token_stale \|\| is_session_invalid`；`pod_util_metrics` 两处调用点补传 namespace / workload；登录校验失败时多打一行 Cookie 完整度。**第四轮**：给 APM / 数据库 / 容器利用率**补上"取到了什么"的日志**（`fmt_metrics`），并去掉重复的「开始统计…」。**第五轮**：新增 `log_ui` 命令（前端日志也落盘）；**跳过视图名非法的 APM 指标**并说明（旧配置残留会导致每个工作负载白打一串 `FailedOperation.ViewNameNotExistOrIllegal`） |
| `src-tauri/src/container.rs` | **容器利用率载荷对齐控制台**：`dimension_json` 补 `namespace`、`workload_name` 两个维度；新增 `pick_period`，`pod_util_metrics` 补 `Period` 字段（详见第二条） |
| `src-tauri/src/login.rs` | csrfCode 只从 `/_api/` 与 `/cgi/capi` 请求取；`cookie_is_complete` 要求 `uin` 非空；预热超时不再打 cred 标记；新增 `describe_cookie`（诊断半套 Cookie，只打有/无）。**第三/四轮**：`read_capi_csrf` → `read_capi_token`（同时带回浏览器 Cookie）；新增 `is_login_url` + `eval_href`，未登录时**快速失败并标 cred**（前端据此自动拉起扫码窗口） |
| `src-tauri/src/capi.rs` | 预热成功后输出一条只含令牌长度的诊断日志。**第三/五轮**：`read_capi_token` 同时带回浏览器 Cookie；新增 `fetch_dashboard`（退路：让隐藏页面自己发请求）；**预热与退路共用 `WINDOW_LOCK` 串行**（并发时它们会互相关掉对方的窗口，表现是 CDP `Connection reset`）；预热日志改走 `log_to_file`（以前用 `app.emit`，只进界面不进日志文件） |
| `docs/DESIGN.md` | 按本文档第二～五节重写为唯一口径 |
| `tools/` | 删除 8 个一次性 `patch_*.py`；新增 `_session.py` / `_capi.py` / `_cdp.py`，重写 `cdp_cookie.py`、`capi_in_browser.py`、`time_container.py`、`time_probe.py`（这些脚本原本**一跑就崩**，详见第十节"已修的脚本坑"）；新增 `smoke_gui.py`（打包自检）、`diagnose_blank_ui.py`（白屏诊断） |
| `src-tauri/Cargo.toml` | 加 `[features] default = ["custom-protocol"]`，让 `cargo build --release` 与 `npx tauri build` 等价（**与白屏无关**，见坑九） |
| `README.md` | 同步到当前模型：删掉 Cookie/密钥双通道、Reqable 抓取、粘贴 cURL、`tc3.rs`/`reqable.rs`；FAQ 里 `code=1216` 不再等同于会话过期 |
| `.github/workflows/release.yml` | 加一步 `cargo test`：那 28 个测试守的是会话/令牌判定的不变量，不跑就等于没有 |
| 单元测试 | 新增 26 个测试（共 28 个，`cargo test` 全绿），把本文档第五节的不变量钉死；`container.rs` 的响应解析抽成纯函数 `parse_util_data`、`capi.rs` 的退路 JS 生成抽成 `build_fetch_js`，便于测试 |
| `.gitignore` | 补 `__pycache__/`、`*.pyc` |

`cargo check` / `cargo build --release` 零警告；`cargo test` 28 个测试全绿；
`dist/apm-monitor.exe` 与 `src-tauri/target/release/apm-monitor.exe` 的 sha256 一致
（`85bf4ad6…`），`dist/config.json` 未动。`python tools/smoke_gui.py` 与
`dist/apm-monitor.log` 的启动标记均已验证（界面渲染**间歇性**受 WebView2 崩溃影响，见坑九）。
工具箱已做本地校验（语法编译、会话派生、载荷形状、CDP 探测），**未**打真实接口。

---

## 九、构建与发布

```bash
# 1) 停掉正在运行的程序（否则 exe 被占用，复制会失败）
# 2) 跑测试（离线，不需要账号）
cd src-tauri && cargo test
# 3) 构建
cargo build --release
# 4) 更新本地绿色版目录（exe 和 DLL 一起）
cp src-tauri/target/release/apm-monitor.exe dist/
cp src-tauri/target/release/WebView2Loader.dll dist/
# 5) 重新打开 dist/apm-monitor.exe（桌面快捷方式指向这里）
```

> ⚠️ **构建后必须确认界面不是白屏**。`Cargo.toml` 的 `default = ["custom-protocol"]` 一旦丢失，
> `cargo build --release` 会产出**能启动但界面全白**的 exe（见坑九）。
> 判断方法：启动后用 CDP 读 `location.href` 与 `document.scripts.length`，
> 白屏时是 `about:blank` + `0`。

**注意**：`dist/config.json` 是使用者的真实配置，升级时**不要覆盖**。前端资源（`ui/`）在编译时嵌入 exe，改了前端必须重新构建，不能只替换 `ui/` 文件。

打绿色版 zip 时**不要把 `config.json` 打进去**——它含会话 Cookie/csrfCode，发出去等于泄露登录态。旧包踩过这个坑，现在 `dist/apm-monitor.zip` 只含 exe + DLL。

发布到 GitHub Release 只认 `v*` 标签（工作流在 `.github/workflows/`）。

---

## 十、验证工具箱（`tools/` 下已备好）

这些都是这次改造过程中写的、可复用的排查脚本。**都是纯 ASCII 的 PowerShell/Python**，直接 `python tools/xxx.py` 或 `powershell -File tools/xxx.ps1` 跑（在仓库根目录跑；脚本自己会把 `tools/` 加进 `sys.path`，不需要 `cd`）。

**不需要 pip 安装任何东西**：CDP 客户端（`_cdp.py`）是纯标准库实现的，只用 `socket`。

### 公共模块（诊断脚本共用，别绕过它们）

| 模块 | 作用 |
|---|---|
| `_session.py` | 读 `dist/config.json` 并按后端口径**从 Cookie 派生 uin/ownerUin**；`flatten_cookies()` 与 `login.rs::build_cookie` 同规则；`headers()` 统一伪装头 |
| `_capi.py` | 两条链路的薄封装：`hc()`（新网关）、`capi()`（旧网关）、`dashboard_body()`（容器利用率请求体） |
| `_cdp.py` | 极简 CDP 客户端（标准库）+ `find_page(port)` 找目标页 |

> **为什么必须有 `_session.py`**：改造后 `uin`/`ownerUin` 已不是 `config.json` 的字段，
> 原来四个脚本都写 `cfg["uin"]`，一跑就 `KeyError: 'uin'`。以后新写诊断脚本请一律走它。

### ⚠️ 不要用 CDP 去"驱动"这个应用

踩过一次：想绕过人工扫码验证容器链路，于是用 CDP 连上主窗口、注入 Cookie、
再 `invoke('query_container_metrics')` 触发统计。结果**预热直接报
`连接不上调试端口 127.0.0.1:9223`** —— 因为**应用自己也要用那个调试端口**（登录/预热全靠它），
外部再占一条 CDP 连接就会互相干扰，测出来的现象是假的。

**结论：容器链路的验证只能走界面**（点「开始统计」），或者用 `verify_container.py`
（它是 UIA 点击 + 读日志文件，不碰 CDP）。**别用 CDP 驱动。**

### 脚本

| 脚本 | 用途 |
|---|---|
| `findwin.py <PID>` | 按进程号列出窗口（拿到主窗口 HWND，后面几个脚本要它） |
| `winenum.py <PID> [秒]` | 列出全部窗口（含子窗口）并轮询新增窗口——查"窗口到底建出来没有" |
| `uia.ps1 -Hwnd <HWND> -Out <文件>` | 用 UI Automation 把界面无障碍树 dump 到文件（能读到 DOM 的 id、文本、`<details>` 展开状态）——本机最可靠的"读界面"手段 |
| `uia_expand.ps1 -Hwnd <HWND>` | 展开日志面板后再 dump |
| `openlogin.ps1 -Hwnd <HWND>` | 展开设置并点「扫码登录」 |
| `startquery.ps1 -Hwnd <HWND>` | 点「开始统计」 |
| `cdp_cookie.py` | 抓登录窗口的 Cookie，并在 T+0/10/20/30/60s 反复试探会话是否可用——查"登录态验证失败是即时失效还是要传播时间" |
| `capi_in_browser.py` | **第二条的主力工具**：先监听 CDP 从页面自己的请求里抓 `x-lid` / `x-life` 基准（`life_epoch`），再在页面里用"页面那份 csrfCode"和"配置里的令牌"各 fetch 一次 dashboard 请求，判定"令牌/头是否对齐"（缺 x-lid/x-life 时页面 fetch 回 `-1`，该脚本已带上，不会误报） |
| `time_container.py` | 容器链路耗时探针：K8s 资源列表 + 旧网关 dashboard（带/不带 pod 过滤各一次），看慢在哪、旧网关回什么 code |
| `time_probe.py` | 数据库/APM 链路耗时探针（`QCE/CMONGO`、`QCE/REDIS_MEM` 等**正确**口径） |
| `smoke_gui.py` | **打包自检（最该先跑的）**：启动应用 → 用 CDP 读 `location.href`/`scripts`/`body` → 截窗口 PNG。判断"包能不能用"的唯一可靠手段（"窗口建出来了"不算） |
| `verify_container.py` | **容器链路的端到端验证**：自动点「开始统计」→ 盯 `dist/apm-monitor.log` → 摘出关键日志并给结论。应用要先打开且已扫码 |
| `diagnose_blank_ui.py` | 白屏诊断：清掉 WebView2 数据目录 → 启动检查 → **正常关闭** → 再启动检查，两次对照。用来证明/修复坑九 |
| `reqable_all.py` | 调 Reqable 的 MCP 把抓包导出到 `tools/capture_dump.json`（本机装了 Reqable 才有用）。**排查容器利用率就是靠它导出的那份抓包** |

> **另外 8 个 `reqable_*.py` 建议删掉**（`reqable_apm/console/curl/filter/find/mcp/state/tke`）：
> 它们是开发期"找接口"的一次性探查脚本（`"""定位「实例分析」数据接口"""`、`"""取 id=99 原始记录结构"""` 这种），
> 写死了 Reqable 的安装路径，能力已被 `reqable_all.py` 覆盖。留着只是噪音——
> 误跑无害（不会改源码），所以没替你做主，要删直接 `git rm tools/reqable_{apm,console,curl,filter,find,mcp,state,tke}.py`。

补充两个查界面时常用的手法（这次实测有效）：`PrintWindow(hwnd, hdc, 2)` 能把被遮挡的窗口内容抓成 PNG；窗口滚轮翻页要给 `PostMessage(renderWidget, WM_MOUSEWHEEL, …)` 传**屏幕坐标**，且两次之间要隔 150ms 以上（Chromium 有滚轮手势锁存，间隔太短会一直粘在内层滚动条上）。

### 已修的脚本坑（第二轮）

| 脚本 | 原来的毛病 | 现在 |
|---|---|---|
| `cdp_cookie.py` / `capi_in_browser.py` / `time_container.py` / `time_probe.py` | `cfg["uin"]`、`cfg["ownerUin"]` → `KeyError`（字段已删） | 走 `_session.session()` |
| `cdp_cookie.py` / `capi_in_browser.py` | `ModuleNotFoundError: No module named 'websocket'` | 换成标准库版 `_cdp.py` |
| `time_container.py` | capi URL 里带 `json=1`（必然 1216） | 去掉；并**真的调用**了原来写了却没被调用的 `capi()` |
| `time_probe.py` | 用了已下线的 `QCE/MONGODB`、`QCE/REDIS` | 改为 `QCE/CMONGO`、`QCE/REDIS_MEM`，维度名也对齐 |
| `cdp_cookie.py` | 扁平化 Cookie 时不跳空值、不优先控制台域（会自己制造"半套 Cookie"） | 与 `build_cookie` 同规则 |
| `capi_in_browser.py` | 依赖一个并不存在的 `tools/_replay_body.json` | 请求体自己按 config 拼，不再需要外部文件 |

---

## 十一、术语表

- **会话 / session**：控制台登录态，即 Cookie + csrfCode 这一对。
- **csrfCode**：控制台接口要求的一个令牌，跟着会话签发。两条链路各有自己认可的那一个。
- **旧网关 / capi**：`console.cloud.tencent.com/cgi/capi`，容器利用率唯一的数据源，令牌寿命特别短。
- **控制台页面自己的令牌**：指页面自己发 `capi` 请求时 URL 里带的那个 csrfCode，和 `/_api` 用的是两码事。
- **预热**：统计前先开一个隐藏窗口让页面把令牌"用"出来，再拿它去发我们自己的请求。
- **半套 Cookie**：抓取时页面还没初始化完就取走的 Cookie 集合（缺 `saas_synced_session` 等），服务端会判 `code=9`。
