import io, sys

p = "ui/app.js"
s = io.open(p, encoding="utf-8").read()
orig = s

def rep(old, new, tag):
    global s
    if old not in s:
        print("MISS:", tag)
        return
    s = s.replace(old, new, 1)
    print("OK:", tag)

# A) 全局状态
rep("""let activeListTab = "apm";
let activeMetricTab = "apm";""",
"""let activeListTab = "apm";
let activeMetricTab = "apm";
let containerClusters = [];
let containerNamespaces = [];""", "globals")

# B) loadSources 支持 container
rep("""    for (const t of types) {
      try {
        if (t === "apm") {
          sourcesData[t] = await invoke("list_apps", { config: cfg });
        } else {""",
"""    for (const t of types) {
      try {
        if (t === "apm") {
          sourcesData[t] = await invoke("list_apps", { config: cfg });
        } else if (t === "container") {
          containerClusters = await invoke("list_clusters", { config: cfg });
          if (!cfg.containerCluster && containerClusters.length) cfg.containerCluster = containerClusters[0].id;
          if (cfg.containerCluster) {
            containerNamespaces = await invoke("list_namespaces", { config: cfg, clusterId: cfg.containerCluster });
            if (!containerNamespaces.includes(cfg.containerNamespace)) cfg.containerNamespace = containerNamespaces[0] || "";
          }
          if (cfg.containerCluster && cfg.containerNamespace) {
            const list = await invoke("list_deployments", { config: cfg, clusterId: cfg.containerCluster, namespace: cfg.containerNamespace });
            sourcesData[t] = list.map((d) => ({ name: d.name, label: d.apmName || "未配 SW_AGENT_NAME", requestCount: 0, meta: d }));
          } else {
            sourcesData[t] = [];
          }
          renderContainerPickers();
        } else {""", "loadSources")

# C) 容器 scope 选择器
rep("""function setAppFilter(selectedOnly) {""",
"""async function reloadContainerScope() {
  cfg.selectedDeployments = [];
  try {
    containerNamespaces = await invoke("list_namespaces", { config: cfg, clusterId: cfg.containerCluster });
    if (!containerNamespaces.includes(cfg.containerNamespace)) cfg.containerNamespace = containerNamespaces[0] || "";
    if (cfg.containerNamespace) {
      const list = await invoke("list_deployments", { config: cfg, clusterId: cfg.containerCluster, namespace: cfg.containerNamespace });
      sourcesData.container = list.map((d) => ({ name: d.name, label: d.apmName || "未配 SW_AGENT_NAME", requestCount: 0, meta: d }));
    } else {
      sourcesData.container = [];
    }
    renderContainerPickers();
    renderApps(true);
    scheduleSave();
  } catch (e) {
    showErr(e);
  }
}

function renderContainerPickers() {
  const box = $("container-pickers");
  if (!box) return;
  const show = activeListTab === "container";
  box.classList.toggle("hidden", !show);
  if (!show) return;
  box.innerHTML = "";
  const mk = (labelText, options, value, onChange) => {
    const label = document.createElement("label");
    label.className = "field";
    label.textContent = labelText;
    const sel = document.createElement("select");
    for (const o of options) {
      const opt = document.createElement("option");
      opt.value = o.value;
      opt.textContent = o.text;
      sel.appendChild(opt);
    }
    sel.value = value || "";
    sel.onchange = () => onChange(sel.value);
    label.appendChild(sel);
    return label;
  };
  box.appendChild(mk("集群", containerClusters.map((c) => ({ value: c.id, text: `${c.name}（${c.id}）` })), cfg.containerCluster,
    (v) => { cfg.containerCluster = v; cfg.containerNamespace = ""; reloadContainerScope(); }));
  box.appendChild(mk("命名空间", containerNamespaces.map((n) => ({ value: n, text: n })), cfg.containerNamespace,
    (v) => { cfg.containerNamespace = v; reloadContainerScope(); }));
}

function setAppFilter(selectedOnly) {""", "pickers")

# D) renderApps 同步渲染选择器
rep("""  const kw = ($("app-search")?.value || "").trim().toLowerCase();
  const list = $("app-list");
  list.innerHTML = "";
  const items = sourcesData[t] || [];""",
"""  const kw = ($("app-search")?.value || "").trim().toLowerCase();
  renderContainerPickers();
  const list = $("app-list");
  list.innerHTML = "";
  const items = sourcesData[t] || [];""", "renderApps-pickers")

# E) 容器结果块
rep("""function renderResults(groups, range) {""",
"""function containerLines(app, range) {
  const v = (n) => {
    const x = app.values.find((y) => y.name === n);
    return x ? x.value : undefined;
  };
  const has = (n) => cfg.selectedMetrics.some((m) => m.name === n && m.view === "container");
  const p = range.dbPrefix;
  const lines = [`**${app.name}**`];
  for (const d of CONTAINER_METRICS) {
    if (!has(d.name)) continue;
    const val = v(d.name);
    if (d.name === "pod_ready") {
      const desired = v("pod_desired");
      lines.push(p + "就绪Pod数：" + (val === undefined ? "-" : val + (desired !== undefined ? "/" + desired : "") + " 个"));
    } else if (d.unit === "%") {
      lines.push(p + d.cn + "：" + (val === undefined ? "-" : (Math.round(val * 10) / 10) + "%"));
    } else {
      lines.push(p + d.cn + "：" + (val === undefined ? "-" : fmtCount(val) + " " + d.unit));
    }
  }
  const secs = Math.max(1, range.end - range.start);
  for (const d of cfg.selectedMetrics.filter((m) => isApmView(m.view))) {
    const label = range.prefix + (d.cn || d.name);
    let text;
    if (d.view === "computed") {
      const req = v("request_count");
      text = d.name === "qps_avg" && req !== undefined ? Math.round((req / secs) * 10) / 10 + " 次/秒" : "-";
    } else {
      const val = v(d.name);
      text = val !== undefined ? fmtValue(d.name, val) : "-";
    }
    lines.push(label + "：" + text);
  }
  if (app.error) lines.push("（" + app.error + "）");
  return lines;
}

function renderResults(groups, range) {""", "containerLines")

rep("""      blocks.push(g.src === "apm" ? apmLines(app, range) : dbLines(app, range, g.src));""",
"""      blocks.push(
        g.src === "apm" ? apmLines(app, range)
          : g.src === "container" ? containerLines(app, range)
            : dbLines(app, range, g.src)
      );""", "renderResults-branch")

# F) query 容器分支
rep("""  for (const t of ["mysql", "redis", "mongodb"]) {
    if (!isEnabled(t)) continue;
    const instances = dbInstances(t);""",
"""  if (isEnabled("container")) {
    if (!cfg.containerCluster || !cfg.containerNamespace) return showErr("容器服务已启用，请先选择集群与命名空间");
    if (!(cfg.selectedDeployments || []).length) return showErr("容器服务已启用，请先选择工作负载");
    if (!cfg.selectedMetrics.some((m) => m.view === "container")) return showErr("容器服务已启用，请先勾选容器指标");
    tasks.push(
      invoke("query_container_metrics", {
        config: cfg,
        clusterId: cfg.containerCluster,
        namespace: cfg.containerNamespace,
        deployments: cfg.selectedDeployments,
        startTs: r.start,
        endTs: r.end,
        apmMetrics: cfg.selectedMetrics.filter((m) => isApmView(m.view)),
      }).then((results) => ({ src: "container", results }))
    );
  }
  for (const t of ["mysql", "redis", "mongodb"]) {
    if (!isEnabled(t)) continue;
    const instances = dbInstances(t);""", "query-container")

# G) 启用容器数据源时默认勾选容器指标
rep("""        if (t !== "apm" && !(cfg.selectedMetrics || []).some((m) => m.view === t)) {
          cfg.selectedMetrics.push(
            ...(DB_DEFAULT_SELECTED[t] || []).map((name) => {
              const def = DB_METRICS[t].find((d) => d.name === name);
              return { ...def, view: t };
            })
          );
        }""",
"""        if (t === "container") {
          if (!(cfg.selectedMetrics || []).some((m) => m.view === t)) {
            cfg.selectedMetrics.push(...CONTAINER_METRICS.map((d) => ({ ...d, view: t })));
          }
        } else if (t !== "apm" && !(cfg.selectedMetrics || []).some((m) => m.view === t)) {
          cfg.selectedMetrics.push(
            ...(DB_DEFAULT_SELECTED[t] || []).map((name) => {
              const def = DB_METRICS[t].find((d) => d.name === name);
              return { ...def, view: t };
            })
          );
        }""", "chip-defaults")

# H) init 迁移
rep("""  // 补充已启用数据源默认勾选的核心数据库指标（新增指标不再自动全选，由用户自行勾选）
  for (const t of ["mysql", "redis", "mongodb"]) {
    if (!isEnabled(t)) continue;""",
"""  if (!Array.isArray(cfg.selectedDeployments)) cfg.selectedDeployments = [];
  if (cfg.containerCluster === undefined) cfg.containerCluster = "";
  if (cfg.containerNamespace === undefined) cfg.containerNamespace = "";
  if (isEnabled("container") && !cfg.selectedMetrics.some((m) => m.view === "container")) {
    cfg.selectedMetrics.push(...CONTAINER_METRICS.map((d) => ({ ...d, view: "container" })));
  }
  // 补充已启用数据源默认勾选的核心数据库指标（新增指标不再自动全选，由用户自行勾选）
  for (const t of ["mysql", "redis", "mongodb"]) {
    if (!isEnabled(t)) continue;""", "init-migration")

if s != orig:
    io.open(p, "w", encoding="utf-8").write(s)
    print("written")
else:
    print("no change")
