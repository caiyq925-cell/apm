import io

# ---------- 后端：默认数据源改为 container ----------
p = "src-tauri/src/config.rs"
s = io.open(p, encoding="utf-8").read()
s = s.replace('''fn default_sources() -> Vec<String> {
    vec!["apm".to_string()]
}''', '''fn default_sources() -> Vec<String> {
    vec!["container".to_string()]
}''')
io.open(p, "w", encoding="utf-8").write(s)

# ---------- 前端 ----------
p = "ui/app.js"
s = io.open(p, encoding="utf-8").read()
orig = s

def rep(old, new, tag):
    global s
    if old not in s:
        print("MISS:", tag); return
    s = s.replace(old, new, 1); print("OK:", tag)

# 1) 监控对象去掉 APM 入口（APM 指标仍在容器服务页签下勾选）
rep('''const SOURCE_ORDER = ["apm", "container", "mysql", "redis", "mongodb"];''',
    '''const SOURCE_ORDER = ["container", "mysql", "redis", "mongodb"];''', "source-order")

# 2) 指标面板：容器页签下分「容器指标」+「APM 指标」两组
rep('''  if (t === "apm") {
    const defs = cfg.metricCache.length ? cfg.metricCache : DEFAULT_METRICS;
    addGroup("应用指标", defs.filter((d) => d.view === "service_metric"));
    addGroup("计算指标", defs.filter((d) => d.view === "computed"));
    addGroup("SQL 调用", defs.filter((d) => d.view === "sql_metric"));
    addGroup("MQ 消息", defs.filter((d) => d.view === "mq_metric"));
    addGroup("JVM 运行时", defs.filter((d) => d.view === "runtime_metric"));
    addGroup("实例指标（多实例取最大）", defs.filter((d) => d.view === "instance_metric"));
  } else {
    addGroup(SOURCE_NAMES[t], DB_METRICS[t].map((d) => ({ ...d, view: t })));
  }''',
'''  if (t === "container") {
    // 容器指标
    addGroup("容器指标", CONTAINER_METRICS.map((d) => ({ ...d, view: "container" })));
    // 关联的 APM 指标（按 SW_AGENT_NAME 严格匹配的应用）
    const defs = cfg.metricCache.length ? cfg.metricCache : DEFAULT_METRICS;
    addGroup("APM 指标（应用指标）", defs.filter((d) => d.view === "service_metric"));
    addGroup("APM 指标（计算）", defs.filter((d) => d.view === "computed"));
    addGroup("APM 指标（实例：多实例取最大）", defs.filter((d) => d.view === "instance_metric"));
    addGroup("APM 指标（SQL 调用）", defs.filter((d) => d.view === "sql_metric"));
    addGroup("APM 指标（MQ 消息）", defs.filter((d) => d.view === "mq_metric"));
    addGroup("APM 指标（JVM 运行时）", defs.filter((d) => d.view === "runtime_metric"));
  } else {
    addGroup(SOURCE_NAMES[t], DB_METRICS[t].map((d) => ({ ...d, view: t })));
  }''', "metric-groups")

# 3) 指标页签角标：容器页签 = 容器指标 + 关联的 APM 指标
rep('''    badge.textContent = cfg.selectedMetrics.filter((m) =>
      t === "apm" ? isApmView(m.view) : m.view === t
    ).length;''',
'''    badge.textContent = cfg.selectedMetrics.filter((m) =>
      t === "container" ? m.view === "container" || isApmView(m.view) : m.view === t
    ).length;''', "metric-badge")

# 4) 去掉独立的 APM 查询分支（APM 指标只随容器服务查询）
rep('''  if (isEnabled("apm")) {
    if (!cfg.instanceId) return showErr("APM 已启用，请先填写业务系统 ID");
    if (!cfg.selectedApps.length) return showErr("APM 已启用，请先选择应用");
    const metrics = cfg.selectedMetrics.filter((m) => isApmView(m.view));
    if (!metrics.length) return showErr("APM 已启用，请先勾选指标");
    tasks.push(
      invoke("query_metrics", {
        config: cfg,
        startTs: r.start,
        endTs: r.end,
        apps: cfg.selectedApps,
        metrics,
      }).then((results) => ({ src: "apm", results }))
    );
  }
''', '', "query-apm-branch")

# 5) 配置迁移：移除 apm 数据源；APM 指标勾选保留（供容器关联使用）
rep('''  if (!cfg.enabledSources || !cfg.enabledSources.length) cfg.enabledSources = ["apm"];''',
'''  if (!Array.isArray(cfg.enabledSources)) cfg.enabledSources = [];
  cfg.enabledSources = cfg.enabledSources.filter((t) => SOURCE_ORDER.includes(t));
  if (!cfg.enabledSources.length) cfg.enabledSources = ["container"];''', "migration-sources")

# 6) 团队 ID 字段恒显示（容器关联 APM 仍需）
rep('''  $("field-team").classList.toggle("hidden", !isEnabled("apm"));''',
    '''  $("field-team").classList.remove("hidden");''', "team-field")

# 7) renderApps 里 activeListTab 兜底不再引用 apm
rep('''  if (!isEnabled(t)) {
    const first = (cfg.enabledSources || []).find(isEnabled);
    if (first) { activeListTab = t = first; renderListTabs(); }
    else return;
  }''',
'''  if (!isEnabled(t)) {
    const first = (cfg.enabledSources || []).find(isEnabled);
    if (first) { activeListTab = t = first; renderListTabs(); }
    else return;
  }''', "renderApps-guard-noop")

# 8) 指标页签初始化默认选中
rep('''  activeListTab = cfg.enabledSources.find(isEnabled) || "apm";
  activeMetricTab = activeListTab;''',
'''  activeListTab = cfg.enabledSources.find(isEnabled) || "container";
  activeMetricTab = activeListTab;''', "init-tabs")

# 9) APM 指标在容器场景下的查询前置校验
rep('''  if (isEnabled("container")) {
    if (!cfg.containerCluster || !cfg.containerNamespace) return showErr("容器服务已启用，请先选择集群与命名空间");''',
'''  if (isEnabled("container")) {
    if (!cfg.instanceId) return showErr("请先在设置中填写业务系统 ID（关联 APM 需要）");
    if (!cfg.containerCluster || !cfg.containerNamespace) return showErr("容器服务已启用，请先选择集群与命名空间");''', "query-precheck")

if s != orig:
    io.open(p, "w", encoding="utf-8").write(s)
    print("written")
