# APM Monitor

腾讯云 APM 多应用指标速查桌面工具（Tauri 2 + Rust + 原生前端）。

## 功能
- 地域选择（默认上海）+ 业务系统 ID（team），自动加载该业务系统下全部应用（按请求量降序，支持搜索过滤、多选）
- 统计时间：最近 15分钟/30分钟/1小时/2小时/6小时/12小时/24小时/今天/昨天/最近3天/最近7天，或自定义起止（精确到分钟）
- 指标清单通过 `DescribeGeneralMetricList` 拉取并缓存，按视图分组勾选；默认勾选：请求量、异常请求量、异常率、P99/P95/平均/最大耗时
- 结果按应用渲染为 Markdown（应用名加粗，逐行"指标名：指标值"），单应用/全部一键复制
- 认证双通道：SecretId/SecretKey（TC3 签名，Rust 原生实现）或 Cookie（手动粘贴 + 手动校验）
- 所有界面状态自动持久化到 **exe 同目录的 config.json**（便携式）

## 开发
```bash
npm install
npm run tauri dev
```

## 构建（单 exe）
```bash
npm run tauri build
# 产物: src-tauri/target/release/apm-monitor.exe，拷贝到任意目录即可
# config.json 会生成在 exe 旁边
```

## 使用注意
- 首次使用需在「设置」中填写：地域、业务系统 ID、SecretId/SecretKey。
- 应用列表 / 指标清单 / 查询 都需要先设置好时间范围。
- 腾讯云 APM 数据仅保留近 30 天；API 限频 20 次/秒（工具内并发限制为 8）。

## Cookie 模式（已实现，按控制台真实协议）
私有接口形态（抓包确认）：
- `POST https://console-hc.cloud.tencent.com/_api/apm/{Action}?timeout=30000&t={毫秒}&uin={uin}&ownerUin={ownerUin}&csrfCode={code}`
- Body：`{"cmd":"{Action}","serviceType":"apm","data":{...OpenAPI 同构参数...},"regionId":4}`

使用步骤：
1. 设置里切换到 Cookie 模式，粘贴 Cookie 整串（uin/ownerUin 自动解析）；
2. 从任意一个控制台请求的 URL 中复制 `csrfCode` 参数值填入；
3. 点「校验 Cookie」（会真实调用一次应用概览接口）。

Cookie / csrfCode 过期后（一般几小时到一天）重新粘贴即可。Cookie 为明文落盘在 exe 旁的 config.json，注意保管。

## 架构预留
- `src-tauri/src/apm.rs` 为 APM 数据源实现；后续加数据库监控（如 DBbrain/云数据库监控）时，新增对应的 `db.rs` 数据源并在命令层扩展即可，前端按"数据源类型"切换展示。

## 已知风险
- `DescribeServiceOverview` / `DescribeGeneralMetricData` 的个别入参形态（时间戳单位、Filters 键名）以官方文档为准，若调用报参数错误，按报错调整 `src-tauri/src/apm.rs` 中的 payload 即可。
