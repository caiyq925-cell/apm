# APM Monitor — 腾讯云 APM + 容器 + 数据库监控工具

轻量桌面工具（Tauri 2 + Rust + 原生前端），把**腾讯云 APM 应用、TKE 容器工作负载、MySQL/Redis/MongoDB** 的监控指标汇总成一份可直接贴群的统计结果。

- 仓库：<https://github.com/caiyq925-cell/apm>
- 架构与接口设计细节：见 [`docs/DESIGN.md`](docs/DESIGN.md)

---

## 一、功能总览

### 1. 数据源（可同时启用，多 Tab 切换）
| 数据源 | 选择粒度 | 取到的指标 |
|---|---|---|
| **容器服务（TKE）** | 集群 → 命名空间 → 工作负载（Deployment）多选 | CPU 最大利用率（占 limit）、内存最大利用率（占 limit，不含 cache）、就绪 Pod 数、单 Pod CPU limit、单 Pod 内存 limit；**并按容器内 `SW_AGENT_NAME` 关联出该服务的 APM 指标** |
| **MySQL** | 实例多选（DBbrain 实例口径） | CPU 最高点、内存最高点、峰值 QPS、峰值 TPS、最高连接数（当前/上限）、连接使用率、活跃线程数、慢查询数、InnoDB 缓存命中率、提交数、回滚数、磁盘使用率 |
| **Redis** | 实例多选 | 健康得分、CPU 使用率、节点最大 CPU、内存使用率、节点最大内存、连接使用率、节点最大连接使用率、读请求命中率、峰值 QPS、连接数、内存使用量、Key 总数、执行错误数、Key 驱逐/过期数、慢查询数、平均/最大/P99 时延、入/出流量 |
| **MongoDB** | 实例多选 | 健康得分、最大/平均 CPU 使用率、内存百分比/最大内存百分比、磁盘使用百分比、Mongos 平均/最大 CPU |

### 2. APM 指标（在容器服务页签下勾选，需 `SW_AGENT_NAME` 严格匹配应用名）
- 应用指标：请求量、异常请求量、错误率、平均耗时、最大耗时、最小耗时、慢调用量、容忍调用量（**口径：`span.kind=server`，与控制台一致**）
- 计算指标：平均(次/秒)（请求量 ÷ 窗口秒数，本地计算）
- 实例指标：CPU 最高点、内存最高点（多实例取最大）
- SQL 调用：SQL 请求数、SQL 错误数、SQL 平均响应时间
- MQ 消息：MQ 请求数、MQ 错误数、MQ 平均响应时间
- JVM 运行时：JVM 已用内存、JVM 最大内存、GC 次数、GC 耗时

### 3. 交互与体验
- **场景预设**：一组「数据源 + 应用/实例/工作负载 + 指标」的快照，一键切换；在场景内改动实时回写该场景
- **统计时间**：最近 15/30 分钟、1/2/6/12/24 小时、今天、昨天、最近 3/7 天，或自定义（精确到分钟）
- **结果输出**：Markdown 渲染（应用/服务名加粗），一键复制**纯文本**（无 `**`），可「复制全部」
- **实时日志面板**：输出查询进度（读了哪个工作负载、在查什么），出错标红、完成标绿
- **停止按钮**：查询中可中断，后端会真正停止派发剩余任务，迟到结果被丢弃
- **会话自动刷新**：容器利用率需要控制台旧网关令牌（短时效），统计前隐藏预热；令牌失败时换新令牌重试一次，整次统计不会因容器利用率失败而中断
- **登录方式**：只有内置扫码登录窗口（腾讯官方页面）；扫码后后端取回并保存 Cookie + csrfCode，前端不持有会话
- **配置便携化**：用户配置存 exe 同目录的 `config.json`；会话字段由后端独有并与用户配置分开合并，保存不会被前端空值覆盖
- **会话维护**：可配置后台自动刷新、提前刷新阈值、失败退避与连续失败上限
- **实时日志**：后端进度事件依赖 `src-tauri/capabilities/default.json` 的 `core:default` 权限

---

## 二、快速开始（使用者）

1. 下载 `apm-monitor_0.1.1_x64-setup.zip`（安装版）或直接使用绿色版目录；
2. 首次打开 → 展开「设置」→ 点唯一的「扫码登录」按钮，在腾讯云官方窗口里完成扫码；
   登录态成功取回后，窗口会关闭，会话由后端保存；
3. 填「业务系统 ID」（APM 的 team，如 `apm-tHxaVZjHH`）；
4. 按需选择数据源与监控对象 → 选时间 → 勾指标 → 点「开始统计」。

> `config.json` 内含控制台会话 Cookie 与 csrfCode，**不要分享或提交到仓库**（`.gitignore` 已排除）。
> `uin` / `ownerUin` 不再单独存成字段，而是每次从 Cookie 派生。

---

## 三、构建与打包（开发者）

### 环境要求
- Node.js ≥ 18、Rust（stable）与 MSVC 工具链、WebView2 运行时（Win10/11 自带）

### 开发调试
```bash
npm install          # 仅安装 @tauri-apps/cli
npm run tauri dev    # 热重载开发
```

### 打包（Windows 单机）
```bash
npm run tauri build     # 或直接 cd src-tauri && cargo build --release（等价）
```
> ⚠️ **界面全白时先别怀疑包**：那是 WebView2 用户数据目录被"强制结束进程"弄坏了。
> 删掉 `%LOCALAPPDATA%\com.local.apmmonitor\EBWebView` 重启即可。
> 自检：`python tools/smoke_gui.py`；调试时**不要用 `taskkill /F`**。
产物：
| 产物 | 路径 |
|---|---|
| 绿色版 exe | `src-tauri/target/release/apm-monitor.exe` |
| 运行库（**必须**与 exe 同目录） | `src-tauri/target/release/WebView2Loader.dll` |
| NSIS 安装包 | `src-tauri/target/release/bundle/nsis/apm-monitor_<版本>_x64-setup.exe` |

安装包使用 `webviewInstallMode: embedBootstrapper`：目标机器缺 WebView2 运行时会自动下载安装。

### 本地包更新（日常使用）
项目把"可直接运行的目录"固定为 `dist/`：

```bash
# 1) 停掉正在运行的程序（否则文件被占用会复制失败）
powershell -Command "Get-Process apm-monitor -ErrorAction SilentlyContinue | Stop-Process -Force"
# 2) 构建
npm run tauri build
# 3) 更新 dist（exe + DLL 必须一起更新）
cp src-tauri/target/release/apm-monitor.exe dist/
cp src-tauri/target/release/WebView2Loader.dll dist/
# 4) 重新打开 dist/apm-monitor.exe（桌面已建快捷方式「APM 监控」）
```
`dist/config.json` 是你的实际配置，**升级时不要覆盖**。

### 发布新版本（全平台）
推一个 `v*` 标签即可，GitHub Actions 会自动构建 Windows / macOS(Intel+ARM) / Linux 并挂到 Release：
```bash
git add -A && git commit -m "..."
git push
git tag v0.1.2 && git push origin v0.1.2
```
工作流文件：`.github/workflows/release.yml`。

### 目录结构
```
apm-monitor/
├── ui/                     # 前端（原生 HTML/CSS/JS，anime.js 本地引入）
│   ├── index.html  app.js  style.css  vendor/anime.umd.min.js
├── src-tauri/
│   ├── src/
│   │   ├── main.rs         # Tauri 入口与命令注册
│   │   ├── commands.rs     # 前端可调用的命令层（查询编排、取消、登录）
│   │   ├── config.rs       # 便携式配置（config.json）+ 场景结构
│   │   ├── cred.rs         # 会话失效与旧网关令牌失效的统一判定
│   │   ├── capi.rs         # 旧网关令牌预热与缓存
│   │   ├── console.rs      # 控制台新网关 BFF（Cookie 模式）
│   │   ├── apm.rs          # APM 指标（应用/实例/SQL/MQ/JVM）
│   │   ├── container.rs    # 容器服务（TKE：集群/命名空间/工作负载/Pod + 利用率）
│   │   ├── db.rs           # MySQL / Redis / MongoDB 指标与实例列表
│   │   └── login.rs        # 内置扫码登录（登录窗口 + CDP 读取会话）
│   ├── capabilities/
│   │   └── default.json    # Tauri v2 权限（含实时日志事件监听）
│   ├── tauri.conf.json     # 应用与打包配置（NSIS + dmg/appimage/deb）
│   └── icons/              # 全平台图标（由 npx tauri icon 生成）
├── dist/                   # 本地绿色版运行目录（已 gitignore）
├── docs/DESIGN.md          # 会话模型、接口与指标设计说明（唯一口径）
├── docs/HANDOFF-会话与登录改造.md # 会话改造交接与验证清单
└── tools/                  # 诊断脚本（CDP、容器/接口排查、Reqable 导出）
```

---

## 四、常见问题

| 现象 | 原因 / 处理 |
|---|---|
| `WebView2Loader.dll 找不到` | 绿色版必须把 exe 与 `WebView2Loader.dll` 放同一目录 |
| 顶部提示 `UnauthorizedOperation.CamNoAuth` | 这是腾讯云服务端的权限/对象问题，不是登录方式；先确认当前扫码账号有对应资源权限 |
| 容器利用率显示 `-` | 该指标只来自旧网关 dashboard；工具会先隐藏预热令牌（并抓 `x-lid`/`x-life` 两个头，抓包对齐），失败时自动换令牌重试一次。若仍失败，先看日志中的预热成功/失败提示（含 `x-lid 已取 / x-life 基准已取` 状态），再跑 `python tools/time_container.py` |
| 旧网关返回 `code=1216` | 它可能是短寿命令牌未被接受，**不等于整机会话失效**；容器失败不会再中断数据库/APM 结果。请求必须带 `x-lid` 与新鲜的 `x-life` 头（工具已自动带）；仍失败先跑 `python tools/capi_in_browser.py` 区分令牌口径与请求载荷问题 |
| 证书/杀软误报 | 未签名安装包的常见误报；可提交微软误报申诉或用绿色版 |
| 界面全白、什么都没有 | WebView2 用户数据目录坏了，**不是包的问题**：删掉 `%LOCALAPPDATA%\com.local.apmmonitor\EBWebView` 重启即可。自检 `python tools/smoke_gui.py` |
| 要排查问题时 | 日志同时写在 `dist/apm-monitor.log`（含启动标记与每条进度），直接看文件比截图快 |
| macOS 首次打开被拦 | 包未签名公证，右键「打开」或在「隐私与安全性」里允许 |
