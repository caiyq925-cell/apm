// APM 监控前端逻辑（多数据源：APM / MySQL / Redis / MongoDB 可同时启用）
const { invoke } = window.__TAURI__.core;
const { animate, stagger } = window.anime;

const REGIONS = [
  ["ap-shanghai", "上海"],
  ["ap-guangzhou", "广州"],
  ["ap-beijing", "北京"],
  ["ap-nanjing", "南京"],
  ["ap-chengdu", "成都"],
  ["ap-chongqing", "重庆"],
  ["ap-shenzhen-fsi", "深圳金融"],
  ["ap-shanghai-fsi", "上海金融"],
  ["ap-beijing-fsi", "北京金融"],
  ["ap-hongkong", "中国香港"],
  ["ap-singapore", "新加坡"],
  ["ap-seoul", "首尔"],
  ["ap-tokyo", "东京"],
  ["ap-silicon-valley", "硅谷"],
  ["ap-frankfurt", "法兰克福"],
];

const QUICK_RANGES = [
  ["15m", "最近15分钟", "15分钟"],
  ["30m", "最近30分钟", "30分钟"],
  ["1h", "最近1小时", "1小时"],
  ["2h", "最近2小时", "2小时"],
  ["6h", "最近6小时", "6小时"],
  ["12h", "最近12小时", "12小时"],
  ["24h", "最近24小时", "24小时"],
  ["today", "今天", "今日"],
  ["yesterday", "昨天", "昨日"],
  ["3d", "最近3天", "3天"],
  ["7d", "最近7天", "7天"],
];

// APM 指标（名称均经控制台接口实测确认；刷新按钮返回后端内置的同款清单）
const DEFAULT_METRICS = [
  { name: "request_count", view: "service_metric", cn: "请求量" },
  { name: "qps_avg", view: "computed", cn: "平均(次/秒)" }, // 计算型：请求量÷窗口秒数
  { name: "error_request_count", view: "service_metric", cn: "异常请求量" },
  { name: "error_req_rate_avg", view: "service_metric", cn: "错误率" },
  { name: "duration_p99", view: "service_metric", cn: "P99耗时" },
  { name: "duration_p95", view: "service_metric", cn: "P95耗时" },
  { name: "duration_avg", view: "service_metric", cn: "平均耗时" },
  { name: "duration_max", view: "service_metric", cn: "最大耗时" },
  { name: "duration_p50", view: "service_metric", cn: "P50耗时" },
  { name: "cpu_usage_percent_top", view: "instance_metric", cn: "CPU最高点" },
  { name: "jvm_heap_usage_percent_top", view: "instance_metric", cn: "内存最高点" },
];

// APM 扩展视图指标（SQL 调用 / MQ / JVM / 实例级，全部实测可用；默认不勾选）
const APM_VIEW_METRICS = [
  { name: "request_count", view: "sql_metric", cn: "SQL请求数" },
  { name: "error_request_count", view: "sql_metric", cn: "SQL错误数" },
  { name: "duration_avg", view: "sql_metric", cn: "SQL平均响应时间" },
  { name: "request_count", view: "mq_metric", cn: "MQ请求数" },
  { name: "error_request_count", view: "mq_metric", cn: "MQ错误数" },
  { name: "duration_avg", view: "mq_metric", cn: "MQ平均响应时间" },
  { name: "jvm_memory_used", view: "runtime_metric", cn: "JVM已用内存" },
  { name: "jvm_memory_max", view: "runtime_metric", cn: "JVM最大内存" },
  { name: "jvm_gc_count", view: "runtime_metric", cn: "GC次数" },
  { name: "jvm_gc_time", view: "runtime_metric", cn: "GC耗时" },
  // 实例分析的两个指标（CPU最高点/内存最高点）已纳入默认勾选，见 DEFAULT_METRICS
];

// APM 侧所有视图
const APM_VIEWS = ["service_metric", "computed", "sql_metric", "mq_metric", "runtime_metric", "instance_metric"];
const isApmView = (v) => APM_VIEWS.includes(v);

// 数据源
const SOURCE_ORDER = ["container", "mysql", "redis", "mongodb"];
const SOURCE_NAMES = { apm: "APM 应用", container: "容器服务", mysql: "MySQL", redis: "Redis", mongodb: "MongoDB" };
// 容器服务指标（TKE，占 limit 口径）
const CONTAINER_METRICS = [
  { name: "cpu_util_limit", cn: "CPU最大利用率", unit: "%" },
  { name: "mem_util_limit", cn: "内存最大利用率", unit: "%" },
  { name: "pod_ready", cn: "Pod数", unit: "pod" },
  { name: "cpu_limit_cores", cn: "CPU limit", unit: "核" },
  { name: "mem_limit_mib", cn: "内存 limit", unit: "MiB" },
];
const DB_TITLES = { mysql: "MySQL 实例", redis: "Redis 实例", mongodb: "MongoDB 实例" };
// 数据库指标目录（全部经真实实例实测可用）
const DB_METRICS = {
  mysql: [
    { name: "cpu", cn: "CPU最高点", unit: "%" },
    { name: "mem", cn: "内存最高点", unit: "%" },
    { name: "qps", cn: "峰值QPS", unit: "次/秒" },
    { name: "tps", cn: "峰值TPS", unit: "次/秒" },
    { name: "conn", cn: "最高连接数", unit: "个" },
    { name: "conn_limit", cn: "连接数上限", unit: "个" },
    { name: "conn_rate", cn: "连接使用率", unit: "%" },
    { name: "threads_running", cn: "活跃线程数", unit: "个" },
    { name: "slow", cn: "慢查询数", unit: "次" },
    { name: "innodb_hit", cn: "InnoDB缓存命中率", unit: "%" },
    { name: "commit", cn: "提交数", unit: "次" },
    { name: "rollback", cn: "回滚数", unit: "次" },
    { name: "disk", cn: "磁盘使用率", unit: "%", noPrefix: true },
  ],
  redis: [
    { name: "score", cn: "健康得分", unit: "分", noPrefix: true },
    { name: "cpu", cn: "CPU使用率", unit: "%" },
    { name: "cpu_max", cn: "节点最大CPU使用率", unit: "%" },
    { name: "mem", cn: "内存使用率", unit: "%" },
    { name: "mem_max", cn: "节点最大内存使用率", unit: "%" },
    { name: "conn_util", cn: "连接使用率", unit: "%" },
    { name: "conn_max_util", cn: "节点最大连接使用率", unit: "%" },
    { name: "hit", cn: "读请求命中率", unit: "%" },
    { name: "qps", cn: "峰值QPS", unit: "次/秒" },
    { name: "conn", cn: "连接数", unit: "个" },
    { name: "mem_used", cn: "内存使用量", unit: "MB" },
    { name: "keys", cn: "Key总数", unit: "个" },
    { name: "cmd_err", cn: "执行错误数", unit: "次" },
    { name: "evicted", cn: "Key驱逐数", unit: "个" },
    { name: "expired", cn: "Key过期数", unit: "个" },
    { name: "slow", cn: "慢查询数", unit: "次" },
    { name: "latency_avg", cn: "平均执行时延", unit: "ms" },
    { name: "latency_max", cn: "最大执行时延", unit: "ms" },
    { name: "latency_p99", cn: "P99执行时延", unit: "ms" },
    { name: "flow_in", cn: "入流量", unit: "MB/s" },
    { name: "flow_out", cn: "出流量", unit: "MB/s" },
  ],
  mongodb: [
    { name: "score", cn: "健康得分", unit: "分", noPrefix: true },
    { name: "cpu", cn: "最大CPU使用率", unit: "%" },
    { name: "cpu_avg", cn: "平均CPU使用率", unit: "%" },
    { name: "mem", cn: "内存百分比", unit: "%" },
    { name: "mem_max", cn: "最大内存百分比", unit: "%" },
    { name: "disk", cn: "磁盘使用百分比", unit: "%", noPrefix: true },
    { name: "mongos_cpu_avg", cn: "Mongos平均CPU使用率", unit: "%", hideIfEmpty: true },
    { name: "mongos_cpu_max", cn: "Mongos最大CPU使用率", unit: "%", hideIfEmpty: true },
  ],
};
// 默认勾选（核心指标），其余可在指标面板自行勾选
const DB_DEFAULT_SELECTED = {
  mysql: ["cpu", "mem", "qps", "conn", "disk"],
  redis: ["score", "cpu", "mem", "conn_util", "hit"],
  mongodb: ["score", "cpu", "mem", "disk"],
};

let cfg = null;
let sourcesData = {}; // {type: [{name, label?, requestCount}]}
let activeListTab = "container";
let activeMetricTab = "container";
let containerClusters = [];
let containerNamespaces = [];
let appFilterSelectedOnly = false;
let saveTimer = null;

const $ = (id) => document.getElementById(id);
const isEnabled = (t) => (cfg.enabledSources || []).includes(t);
const dbInstances = (t) => (cfg.selectedDbInstances && cfg.selectedDbInstances[t]) || [];

// ---------- 配置持久化 ----------
function snapshotSelection() {
  return {
    sources: [...cfg.enabledSources],
    apps: [...cfg.selectedApps],
    dbInstances: JSON.parse(JSON.stringify(cfg.selectedDbInstances || {})),
    metrics: JSON.parse(JSON.stringify(cfg.selectedMetrics || [])),
    deployments: [...(cfg.selectedDeployments || [])],
  };
}

function applySelection(snap) {
  // 数据源：场景显式记录的 + 从内容反推的（兼容早期没有 sources 字段的老场景）
  const sources = new Set((snap.sources || []).filter(Boolean));
  if ((snap.apps || []).length) sources.add("apm");
  for (const [k, v] of Object.entries(snap.dbInstances || {})) {
    if ((v || []).length) sources.add(k);
  }
  if (sources.size) cfg.enabledSources = [...sources];
  cfg.selectedApps = [...(snap.apps || [])];
  cfg.selectedDbInstances = JSON.parse(JSON.stringify(snap.dbInstances || {}));
  cfg.selectedDeployments = [...(snap.deployments || [])];
  // 指标定义以当前版本目录为准补齐（老场景缺单位/展示标记时自动修正）
  cfg.selectedMetrics = (snap.metrics || []).map((m) => {
    const cat = (DB_METRICS[m.view] || []).find((d) => d.name === m.name);
    if (cat) return { ...cat, view: m.view };
    const apm = [...DEFAULT_METRICS, ...APM_VIEW_METRICS].find((d) => d.name === m.name && d.view === m.view);
    return apm ? { ...apm } : { ...m };
  });
}

// 激活场景时，界面上的一切勾选改动实时回写到场景
function syncActiveScenario() {
  if (!cfg.activeScenario || cfg.activeScenario === "自定义") return;
  const s = (cfg.scenarios || []).find((x) => x.name === cfg.activeScenario);
  if (!s) return;
  const snap = snapshotSelection();
  s.sources = snap.sources;
  s.apps = snap.apps;
  s.dbInstances = snap.dbInstances;
  s.metrics = snap.metrics;
  s.deployments = snap.deployments;
}

function scheduleSave() {
  syncActiveScenario();
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => invoke("save_config", { config: cfg }).catch(showErr), 400);
}

// ---------- 场景 ----------
function renderScenarioChips() {
  const box = $("scenario-chips");
  box.innerHTML = "";
  const active = cfg.activeScenario || "自定义";
  const names = ["自定义", ...(cfg.scenarios || []).map((s) => s.name)];
  for (const name of names) {
    const chip = document.createElement("button");
    chip.className = "chip" + (active === name ? " active" : "");
    let label = name;
    if (name !== "自定义") {
      const s = cfg.scenarios.find((x) => x.name === name);
      if (s) {
        const src = (s.sources || []).map((k) => SOURCE_NAMES[k] || k).join(" ");
        label += `（${src || "自定义"} · 应用 ${s.apps.length} · 库 ${(Object.values(s.dbInstances || {})).reduce((n, v) => n + v.length, 0)}）`;
      }
    }
    chip.textContent = label;
    chip.onclick = () => switchScenario(name);
    box.appendChild(chip);
  }
  $("btn-scenario-delete").disabled = active === "自定义";
  $("btn-scenario-save").textContent = active === "自定义" ? "存为场景" : "保存场景";
}

function switchScenario(name) {
  if ((cfg.activeScenario || "自定义") === name) return;
  // 把当前工作区写回原场景，避免丢失改动
  if (cfg.activeScenario && cfg.activeScenario !== "自定义") {
    const prev = (cfg.scenarios || []).find((x) => x.name === cfg.activeScenario);
    if (prev) {
      const snap = snapshotSelection();
      prev.sources = snap.sources;
      prev.apps = snap.apps;
      prev.dbInstances = snap.dbInstances;
      prev.metrics = snap.metrics;
      prev.deployments = snap.deployments;
    }
  }
  cfg.activeScenario = name === "自定义" ? "" : name;
  const s = (cfg.scenarios || []).find((x) => x.name === name);
  if (s) applySelection(s);
  // 清掉搜索词与「已选」筛选，保证场景里的对象立刻可见
  $("app-search").value = "";
  setAppFilter(false);
  renderScenarioChips();
  renderSourceChips();
  renderTabs();
  renderApps(false);
  renderMetrics();
  updateSettingsHint();
  scheduleSave();
  // 场景切换后，自动加载该场景启用但尚未拉取过列表的数据源
  if ((cfg.cookie || cfg.secretId)) {
    const missing = cfg.enabledSources.filter((t) => isEnabled(t) && !sourcesData[t]);
    if (missing.length) loadSources(missing);
  }
}

function saveScenario() {
  const snap = snapshotSelection();
  // 已激活命名场景：直接保存（覆盖），不再弹命名
  if (cfg.activeScenario && cfg.activeScenario !== "自定义") {
    const s = cfg.scenarios.find((x) => x.name === cfg.activeScenario);
    if (s) {
      s.sources = snap.sources;
      s.apps = snap.apps;
      s.dbInstances = snap.dbInstances;
      s.metrics = snap.metrics;
      s.deployments = snap.deployments;
      renderScenarioChips();
      scheduleSave();
      showOk(`场景「${s.name}」已保存`);
      return;
    }
  }
  let name = window.prompt("场景名称：", "");
  name = (name || "").trim();
  if (!name || name === "自定义") return;
  if (!cfg.scenarios) cfg.scenarios = [];
  const exist = cfg.scenarios.find((x) => x.name === name);
  if (exist) {
    exist.sources = snap.sources;
    exist.apps = snap.apps;
    exist.dbInstances = snap.dbInstances;
    exist.metrics = snap.metrics;
    exist.deployments = snap.deployments;
  } else {
    cfg.scenarios.push({ name, sources: snap.sources, apps: snap.apps, dbInstances: snap.dbInstances, metrics: snap.metrics, deployments: snap.deployments });
  }
  cfg.activeScenario = name;
  renderScenarioChips();
  scheduleSave();
  showOk(`场景「${name}」已保存${exist ? "（覆盖）" : ""}`);
}

function deleteScenario() {
  const name = cfg.activeScenario;
  if (!name || name === "自定义") return;
  if (!window.confirm(`删除场景「${name}」？`)) return;
  cfg.scenarios = cfg.scenarios.filter((x) => x.name !== name);
  cfg.activeScenario = "";
  renderScenarioChips();
  scheduleSave();
  showOk(`场景「${name}」已删除`);
}

function showErr(e) {
  $("status").classList.remove("ok");
  $("status").textContent = String(e);
}

function showOk(msg) {
  $("status").classList.add("ok");
  $("status").textContent = msg;
  setTimeout(() => { if ($("status").textContent === msg) $("status").textContent = ""; }, 5000);
}

// ---------- 时间 ----------
function resolveRange() {
  const now = Math.floor(Date.now() / 1000);
  const day = (offsetDays) => {
    const d = new Date();
    d.setHours(0, 0, 0, 0);
    return Math.floor(d.getTime() / 1000) + offsetDays * 86400;
  };
  const dur = { "15m": 900, "30m": 1800, "1h": 3600, "2h": 7200, "6h": 21600, "12h": 43200, "24h": 86400, "3d": 259200, "7d": 604800 };
  if (cfg.useCustom) {
    const s = Math.floor(new Date(cfg.customStart).getTime() / 1000);
    const e = Math.floor(new Date(cfg.customEnd).getTime() / 1000);
    if (!s || !e || s >= e) return null;
    return { start: s, end: e, label: `${cfg.customStart.replace("T", " ")} ~ ${cfg.customEnd.replace("T", " ")}`, prefix: "", dbPrefix: "" };
  }
  if (cfg.quickRange === "today") return { start: day(0), end: now, label: "今天", prefix: "今日", dbPrefix: "今日" };
  if (cfg.quickRange === "yesterday") return { start: day(-1), end: day(0), label: "昨天", prefix: "昨日", dbPrefix: "昨日" };
  const d = dur[cfg.quickRange];
  if (!d) return null;
  const q = QUICK_RANGES.find((x) => x[0] === cfg.quickRange);
  return { start: now - d, end: now, label: q[1], prefix: q[2], dbPrefix: `近${q[2]}` };
}

function updateRangeHint() {
  const r = resolveRange();
  $("range-hint").textContent = r ? `查询区间：${r.label}` : "请先设置有效的时间范围";
}

// ---------- 数据源 chips ----------
function renderSourceChips() {
  const box = $("source-chips");
  box.innerHTML = "";
  for (const t of SOURCE_ORDER) {
    const chip = document.createElement("button");
    chip.className = "chip" + (isEnabled(t) ? " active" : "");
    chip.textContent = SOURCE_NAMES[t];
    chip.onclick = () => {
      const i = cfg.enabledSources.indexOf(t);
      if (i >= 0) {
        if (cfg.enabledSources.length === 1) return showErr("至少保留一个数据源");
        cfg.enabledSources.splice(i, 1);
        if (!isEnabled(activeListTab)) activeListTab = cfg.enabledSources[0];
        if (!isEnabled(activeMetricTab)) activeMetricTab = cfg.enabledSources[0];
      } else {
        cfg.enabledSources.push(t);
        if (t === "container") {
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
        }
      }
      renderSourceChips();
      renderTabs();
      renderApps(false);
      renderMetrics();
      updateSettingsHint();
      scheduleSave();
      if (isEnabled(t) && !sourcesData[t] && (cfg.cookie || cfg.secretId)) loadSources([t]);
    };
    box.appendChild(chip);
  }
}

// ---------- Tab 页 ----------
function renderTabs() {
  renderListTabs();
  renderMetricTabs();
}

function renderListTabs() {
  const box = $("list-tabs");
  box.innerHTML = "";
  for (const t of SOURCE_ORDER) {
    if (!isEnabled(t)) continue;
    const btn = document.createElement("button");
    btn.className = "tab" + (t === activeListTab ? " active" : "");
    const badge = document.createElement("span");
    badge.className = "badge";
    badge.textContent = selectedListOf(t).length;
    btn.appendChild(document.createTextNode(SOURCE_NAMES[t]));
    btn.appendChild(badge);
    btn.onclick = () => {
      activeListTab = t;
      // 右侧指标 Tab 跟随左侧切换
      activeMetricTab = t;
      renderListTabs();
      renderMetricTabs();
      renderMetrics();
      renderApps(false);
      if (!sourcesData[t] && (cfg.cookie || cfg.secretId)) loadSources([t]);
    };
    box.appendChild(btn);
  }
}

function renderMetricTabs() {
  const box = $("metric-tabs");
  box.innerHTML = "";
  for (const t of SOURCE_ORDER) {
    if (!isEnabled(t)) continue;
    const btn = document.createElement("button");
    btn.className = "tab" + (t === activeMetricTab ? " active" : "");
    const badge = document.createElement("span");
    badge.className = "badge";
    badge.textContent = cfg.selectedMetrics.filter((m) =>
      t === "container" ? m.view === "container" || isApmView(m.view) : m.view === t
    ).length;
    btn.appendChild(document.createTextNode(SOURCE_NAMES[t]));
    btn.appendChild(badge);
    btn.onclick = () => {
      activeMetricTab = t;
      renderMetricTabs();
      renderMetrics();
    };
    box.appendChild(btn);
  }
}

// ---------- 应用 / 实例列表 ----------
async function loadSources(types) {
  $("btn-load-apps").disabled = true;
  let okCount = 0;
  try {
    for (const t of types) {
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
        } else {
          const list = await invoke("list_db_instances", { config: cfg, dbType: t });
          sourcesData[t] = list.map((d) => ({ name: d.id, label: d.name, requestCount: 0 }));
        }
        okCount++;
      } catch (e) {
        showErr(`${SOURCE_NAMES[t]}: ${e}`);
      }
    }
    renderApps(false);
    renderListTabs();
    if (okCount) showOk(`已加载 ${okCount} 个数据源的列表`);
  } finally {
    $("btn-load-apps").disabled = false;
  }
}

function selectedListOf(t) {
  if (t === "apm") return cfg.selectedApps;
  if (t === "container") return cfg.selectedDeployments || (cfg.selectedDeployments = []);
  return dbInstances(t);
}

function toggleSelected(t, name, checked) {
  const list = selectedListOf(t);
  const i = list.indexOf(name);
  if (checked && i < 0) list.push(name);
  if (!checked && i >= 0) list.splice(i, 1);
  if (t === "container") {
    cfg.selectedDeployments = list;
  } else if (t !== "apm") {
    cfg.selectedDbInstances = cfg.selectedDbInstances || {};
    cfg.selectedDbInstances[t] = list;
  }
  $("selected-app-count").textContent = selectedListOf(activeListTab).length;
  renderListTabs();
  scheduleSave();
}

async function reloadContainerScope() {
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

  // 命名空间：可输入搜索的下拉（datalist）
  const nsLabel = document.createElement("label");
  nsLabel.className = "field";
  nsLabel.textContent = "命名空间（可输入搜索）";
  const datalistId = "ns-options";
  const input = document.createElement("input");
  input.setAttribute("list", datalistId);
  input.placeholder = "输入关键字筛选…";
  input.value = cfg.containerNamespace || "";
  input.style.minWidth = "200px";
  const dl = document.createElement("datalist");
  dl.id = datalistId;
  for (const n of containerNamespaces) {
    const opt = document.createElement("option");
    opt.value = n;
    dl.appendChild(opt);
  }
  const apply = () => {
    const v = input.value.trim();
    if (!containerNamespaces.includes(v)) return; // 未匹配到就不动
    if (v === cfg.containerNamespace) return;
    cfg.containerNamespace = v;
    reloadContainerScope();
  };
  input.onchange = apply;
  input.onkeydown = (e) => { if (e.key === "Enter") apply(); };
  nsLabel.appendChild(input);
  nsLabel.appendChild(dl);
  box.appendChild(nsLabel);
}

function setAppFilter(selectedOnly) {
  appFilterSelectedOnly = selectedOnly;
  $("btn-app-filter-all").classList.toggle("active", !selectedOnly);
  $("btn-app-selected").classList.toggle("active", selectedOnly);
  renderApps(false);
}

function renderApps(animateIn) {
  const t = activeListTab;
  if (!isEnabled(t)) {
    const first = (cfg.enabledSources || []).find(isEnabled);
    if (first) { activeListTab = t = first; renderListTabs(); }
    else return;
  }
  const kw = ($("app-search")?.value || "").trim().toLowerCase();
  renderContainerPickers();
  const list = $("app-list");
  list.innerHTML = "";
  const items = sourcesData[t] || [];
  let shown = items.filter((a) => (a.name + (a.label || "")).toLowerCase().includes(kw));
  if (appFilterSelectedOnly) {
    const sel = selectedListOf(t);
    shown = shown.filter((a) => sel.includes(a.name));
  }
  for (const a of shown) {
    const row = document.createElement("label");
    row.className = "check-item";
    const cb = document.createElement("input");
    cb.type = "checkbox";
    cb.checked = selectedListOf(t).includes(a.name);
    cb.onchange = () => toggleSelected(t, a.name, cb.checked);
    row.appendChild(cb);
    const name = document.createElement("span");
    name.textContent = t !== "apm" && a.label ? `${a.name}（${a.label}）` : a.name;
    row.appendChild(name);
    const cnt = document.createElement("span");
    cnt.className = "cnt";
    cnt.textContent = t === "apm" ? `${fmtCount(a.requestCount)} 次` : "";
    row.appendChild(cnt);
    list.appendChild(row);
  }
  if (!shown.length) {
    const empty = document.createElement("div");
    empty.className = "muted";
    empty.style.padding = "8px";
    empty.textContent = appFilterSelectedOnly
      ? "当前页签还没有勾选任何条目"
      : items.length
        ? "没有匹配的条目"
        : `${SOURCE_NAMES[t]} 列表未加载，点「加载列表」拉取`;
    list.appendChild(empty);
  }
  $("selected-app-count").textContent = selectedListOf(t).length;
  if (animateIn && shown.length) {
    animate(".check-item", {
      translateY: [8, 0],
      opacity: [0, 1],
      delay: stagger(4),
      duration: 280,
      ease: "outQuad",
    });
  }
}

// ---------- 指标 ----------
function renderMetrics() {
  const t = activeMetricTab;
  if (!isEnabled(t)) {
    const first = (cfg.enabledSources || []).find(isEnabled);
    if (first) { activeMetricTab = t = first; renderMetricTabs(); }
    else return;
  }
  const list = $("metric-list");
  list.innerHTML = "";
  const addGroup = (title, defs) => {
    if (!defs.length) return;
    const g = document.createElement("div");
    g.className = "metric-group";
    g.textContent = title;
    list.appendChild(g);
    for (const d of defs) {
      const row = document.createElement("label");
      row.className = "check-item";
      const cb = document.createElement("input");
      cb.type = "checkbox";
      cb.checked = cfg.selectedMetrics.some((m) => m.name === d.name && m.view === d.view);
      cb.onchange = () => {
        if (cb.checked) {
          cfg.selectedMetrics.push({ ...d });
        } else {
          cfg.selectedMetrics = cfg.selectedMetrics.filter((m) => !(m.name === d.name && m.view === d.view));
        }
        renderMetricTabs();
        scheduleSave();
      };
      row.appendChild(cb);
      const label = document.createElement("span");
      label.textContent = t === "apm" ? `${d.cn || d.name} (${d.name})` : d.cn;
      row.appendChild(label);
      list.appendChild(row);
    }
  };
  if (t === "container") {
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
  }
}

async function refreshMetrics() {
  $("btn-refresh-metrics").disabled = true;
  try {
    const defs = await invoke("refresh_metric_defs", { config: cfg });
    if (!defs.length) throw "指标清单为空";
    cfg.metricCache = defs;
    cfg.selectedMetrics = cfg.selectedMetrics.filter((m) => !isApmView(m.view) || defs.some((d) => d.name === m.name && d.view === m.view));
    renderMetrics();
    scheduleSave();
    showOk(`指标清单已刷新，共 ${defs.length} 个`);
  } catch (e) {
    showErr(e);
  } finally {
    $("btn-refresh-metrics").disabled = false;
  }
}

// ---------- 数值格式化 ----------
function fmtCount(v) {
  if (!isFinite(v)) return String(v);
  if (Math.abs(v) >= 10000) {
    const w = v / 10000;
    return `${(Math.round(w * 10) / 10).toString().replace(/\.0$/, "")}W`;
  }
  return Number.isInteger(v) ? String(v) : String(Math.round(v * 10) / 10);
}

function fmtRate(v) {
  if (!isFinite(v)) return String(v);
  return `${v.toFixed(2)}%`;
}

function fmtValue(name, v) {
  if (name.includes("rate")) return fmtRate(v);
  if (name.includes("percent")) return `${Math.round(v * 10) / 10}%`;
  if (name.startsWith("duration")) return `${Math.round(v * 10) / 10} ms`;
  if (name.startsWith("jvm_memory")) return `${fmtCount(v)} MB`;
  return fmtCount(v);
}

// ---------- 查询与 Markdown 展示 ----------
function mdEscape(s) {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

function mdRender(md) {
  // 极简 Markdown：**加粗** + 换行
  return mdEscape(md)
    .replace(/\*\*(.+?)\*\*/g, "<b>$1</b>")
    .replace(/\n/g, "<br>");
}

async function query() {
  const r = resolveRange();
  if (!r) return showErr("时间范围无效");

  const tasks = [];
  if (isEnabled("container")) {
    if (!cfg.instanceId) return showErr("请先在设置中填写业务系统 ID（关联 APM 需要）");
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
    const instances = dbInstances(t);
    if (!instances.length) return showErr(`${SOURCE_NAMES[t]} 已启用，请先选择实例`);
    if (!cfg.selectedMetrics.some((m) => m.view === t)) return showErr(`${SOURCE_NAMES[t]} 已启用，请先勾选指标`);
    tasks.push(
      invoke("query_db_metrics", {
          config: cfg,
          dbType: t,
          startTs: r.start,
          endTs: r.end,
          instances,
          metrics: cfg.selectedMetrics.filter((m) => m.view === t).map((m) => m.name),
        }).then((results) => ({ src: t, results }))
    );
  }
  if (!tasks.length) return showErr("请先启用数据源并选择对象");

  for (const id of ["btn-query", "btn-query-2"]) {
    const b = $(id);
    b.disabled = true;
    b.classList.add("loading");
  }
  $("status").textContent = "查询中…";
  try {
    const settled = await Promise.allSettled(tasks);
    const groups = [];
    for (const s of settled) {
      if (s.status === "fulfilled") groups.push(s.value);
      else groups.push({ src: "error", results: [], error: String(s.reason) });
    }
    renderResults(groups, r);
    const total = groups.reduce((n, g) => n + g.results.length, 0);
    showOk(`统计完成：${total} 个对象`);
  } catch (e) {
    showErr(e);
  } finally {
    for (const id of ["btn-query", "btn-query-2"]) {
      const b = $(id);
      b.disabled = false;
      b.classList.remove("loading");
    }
  }
}

function dbLines(app, range, src) {
  const v = (n) => {
    const x = app.values.find((y) => y.name === n);
    return x ? x.value : undefined;
  };
  const pct = (x) => (x === undefined ? "-" : `${Math.round(x * 10) / 10}%`);
  const fmtDb = (d, x) => {
    if (x === undefined) return "-";
    switch (d.unit) {
      case "%": return pct(x);
      case "分": return `${Math.round(x)} 分`;
      case "ms": return `${Math.round(x * 10) / 10} ms`;
      case "次/秒": case "MB/s": return `${Math.round(x * 10) / 10} ${d.unit}`;
      default: return `${fmtCount(x)} ${d.unit}`;
    }
  };
  const has = (n) => cfg.selectedMetrics.some((m) => m.name === n && m.view === src);
  const p = range.dbPrefix;
  const defs = DB_METRICS[src];
  const lines = [`**${app.name}**`];

  // MySQL 连接数特殊格式：当前/上限
  if (src === "mysql" && has("conn")) {
    const c = v("conn");
    const cl = v("conn_limit");
    const txt =
      c === undefined
        ? "-"
        : cl !== undefined
          ? `${fmtCount(c)}/${fmtCount(cl)} 个`
          : `${fmtCount(c)} 个`;
    lines.push(`${p}最高连接数：${txt}`);
  }

  for (const d of defs) {
    if (!has(d.name)) continue;
    if (src === "mysql" && (d.name === "conn" || d.name === "conn_limit")) continue; // 已在上面合并输出
    const val = v(d.name);
    // 标记 hideIfEmpty 的指标（如副本集无 Mongos）在无数据时整行不输出
    if (d.hideIfEmpty && val === undefined) continue;
    lines.push(`${d.noPrefix ? "" : p}${d.cn}：${fmtDb(d, val)}`);
  }
  if (app.error) lines.push(`（查询出错：${app.error}）`);
  return lines;
}

function apmLines(app, range) {
  const lines = [`**服务名：${app.name}**`];
  const secs = Math.max(1, range.end - range.start);
  for (const d of cfg.selectedMetrics) {
    if (!isApmView(d.view)) continue;
    const label = `${range.prefix}${d.cn || d.name}`;
    let text;
    if (d.view === "computed") {
      if (d.name === "qps_avg") {
        const req = app.values.find((x) => x.name === "request_count");
        text = req !== undefined ? `${Math.round((req.value / secs) * 10) / 10} 次/秒` : "-";
      } else {
        text = "-";
      }
    } else {
      const v = app.values.find((x) => x.name === d.name);
      text = v !== undefined ? fmtValue(d.name, v.value) : "-";
    }
    lines.push(`${label}：${text}`);
  }
  if (app.error) lines.push(`（查询出错：${app.error}）`);
  return lines;
}

function containerLines(app, range) {
  const v = (n) => {
    const x = app.values.find((y) => y.name === n);
    return x ? x.value : undefined;
  };
  const has = (n) => cfg.selectedMetrics.some((m) => m.name === n && m.view === "container");
  const p = range.dbPrefix;
  const lines = [`**服务名：${app.name}**`];
  const pct = (n) => {
    const x = v(n);
    return x === undefined ? "-" : `${Math.round(x * 10) / 10}%`;
  };
  // 1) 容器利用率（带时间前缀）
  if (has("cpu_util_limit")) lines.push(`${p}CPU最大利用率：${pct("cpu_util_limit")}`);
  if (has("mem_util_limit")) lines.push(`${p}内存最大利用率：${pct("mem_util_limit")}`);
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
  // 2) 容器规格与副本数（不带时间前缀）
  const ready = v("pod_ready");
  if (has("pod_ready") && ready !== undefined) lines.push(`Pod数：${fmtCount(ready)}个`);
  const cpuLim = v("cpu_limit_cores");
  if (has("cpu_limit_cores") && cpuLim !== undefined) lines.push(`CPU：${fmtCount(cpuLim)} 核`);
  const memLim = v("mem_limit_mib");
  if (has("mem_limit_mib") && memLim !== undefined) {
    const txt = memLim >= 1024 ? `${Math.round((memLim / 1024) * 10) / 10}GB`.replace(".0GB", "GB") : `${fmtCount(memLim)}MB`;
    lines.push(`内存：${txt}`);
  }
  if (app.error) lines.push("（" + app.error + "）");
  return lines;
}

function renderResults(groups, range) {
  const box = $("results");
  box.innerHTML = "";
  const blocks = [];
  for (const g of groups) {
    if (g.src === "error") {
      blocks.push([`**（${g.error}）**`]);
      continue;
    }
    for (const app of g.results) {
      blocks.push(
        g.src === "apm" ? apmLines(app, range)
          : g.src === "container" ? containerLines(app, range)
            : dbLines(app, range, g.src)
      );
    }
  }
  for (const lines of blocks) {
    const md = lines.join("\n");
    // 复制输出为纯文本（去掉 Markdown 加粗符号）
    const plain = lines.map((l) => l.replace(/\*\*/g, "")).join("\n");

    const block = document.createElement("div");
    block.className = "result-block";
    const html = document.createElement("div");
    html.dataset.md = plain;
    html.innerHTML = mdRender(md);
    block.appendChild(html);
    const btn = document.createElement("button");
    btn.className = "btn-mini copy";
    btn.textContent = "复制";
    btn.onclick = () => copyText(plain, btn);
    block.appendChild(btn);
    box.appendChild(block);
  }
  animate(".result-block", {
    translateY: [24, 0],
    opacity: [0, 1],
    scale: [0.98, 1],
    delay: stagger(90),
    duration: 500,
    ease: "outExpo",
  });
}

async function copyText(text, btn) {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const ta = document.createElement("textarea");
    ta.value = text;
    document.body.appendChild(ta);
    ta.select();
    document.execCommand("copy");
    ta.remove();
  }
  if (btn) {
    btn.textContent = "已复制";
    btn.style.color = "var(--ok)";
    setTimeout(() => { btn.textContent = "复制"; btn.style.color = ""; }, 1500);
  }
}

// ---------- curl 粘贴解析 ----------
// 兼容 Windows cmd 版（^" ^& ^% 转义）与 bash 版（"..." '...'）两种 Copy as cURL 格式
function parseCurl(text) {
  // 去掉 cmd 转义符 ^（^\^" → \" 恰好是正确的引号转义）
  const s = text.replace(/\r?\n\s*/g, " ").replace(/\^(.)/g, "$1");
  const toks = [];
  let i = 0;
  while (i < s.length) {
    while (s[i] === " ") i++;
    if (i >= s.length) break;
    let cur = "";
    while (i < s.length && s[i] !== " ") {
      const c = s[i];
      if (c === '"') {
        i++;
        while (i < s.length && s[i] !== '"') {
          if (s[i] === "\\" && (s[i + 1] === '"' || s[i + 1] === "\\")) { cur += s[i + 1]; i += 2; }
          else { cur += s[i]; i++; }
        }
        i++;
      } else if (c === "'") {
        i++;
        while (i < s.length && s[i] !== "'") { cur += s[i]; i++; }
        i++;
      } else { cur += c; i++; }
    }
    toks.push(cur);
  }

  const out = { url: "", cookie: "", team: "", rid: "", isApmApi: false };
  for (let k = 0; k < toks.length; k++) {
    const t = toks[k];
    if (t === "--url" && toks[k + 1]) { out.url = toks[k + 1]; k++; }
    else if (t === "-b" || t === "--cookie") { out.cookie = toks[k + 1] || ""; k++; }
    else if (t === "-H") {
      const h = toks[k + 1] || ""; k++;
      const m = h.match(/^cookie:\s*(.+)$/i);
      if (m) out.cookie = m[1].trim();
    } else if (t === "curl" || t.startsWith("-")) { /* skip */ }
    else if (!out.url) out.url = t;
  }
  if (!out.url) return out;

  let params;
  try { params = new URL(out.url).searchParams; } catch { return out; }
  out.isApmApi = /console-hc\.cloud\.tencent\.com\/_api\/apm\//.test(out.url);
  out.uin = params.get("uin") || "";
  out.ownerUin = params.get("ownerUin") || "";
  out.csrfCode = params.get("csrfCode") || "";

  // team / rid 藏在 URL 自身、referer 头或 from 参数里（可能多层编码），在全文本中搜
  const hunt = (name) => {
    const direct = params.get(name);
    if (direct) return direct;
    const deep = decodeURIComponentSafe(s).match(new RegExp(`[?&]${name}=([^&"'\\s]+)`));
    return deep ? deep[1] : "";
  };
  out.team = hunt("team");
  out.rid = hunt("rid");
  return out;
}

function decodeURIComponentSafe(s) {
  try { return decodeURIComponent(s); } catch { return s; }
}

function parseCurlFill() {
  const text = $("curl-paste").value;
  if (!text.trim()) { $("curl-result").textContent = "请先粘贴 curl"; return; }
  const r = parseCurl(text);

  if (r.cookie) { $("cookie").value = r.cookie; cfg.cookie = r.cookie; }
  if (r.uin) $("uin").value = r.uin;
  if (r.ownerUin) $("owner-uin").value = r.ownerUin;
  if (r.csrfCode) $("csrf-code").value = r.csrfCode;
  if (!r.uin || !r.ownerUin) { const p = parseCookieIds(cfg.cookie || ""); if (!$("uin").value && p.uin) $("uin").value = p.uin; if (!$("owner-uin").value && p.ownerUin) $("owner-uin").value = p.ownerUin; }
  if (r.team) $("instance-id").value = r.team;
  if (r.rid) $("region-id").value = parseInt(r.rid) || cfg.regionId;
  cfg.uin = $("uin").value.trim();
  cfg.ownerUin = $("owner-uin").value.trim();
  cfg.csrfCode = $("csrf-code").value.trim();
  cfg.instanceId = $("instance-id").value.trim();
  cfg.regionId = parseInt($("region-id").value) || 4;
  scheduleSave();

  const has = (v) => (v ? "✓" : "✗");
  let msg = `Cookie ${has(r.cookie)}　uin ${has(cfg.uin)}　ownerUin ${has(cfg.ownerUin)}　csrfCode ${has(cfg.csrfCode)}　team ${cfg.instanceId ? "✓ " + cfg.instanceId : "✗"}　regionId ${cfg.regionId}`;
  if (!r.isApmApi && !r.cookie) {
    msg += "　⚠ 这不是 _api/apm/ 接口请求且提不到 Cookie：请在 F12 网络面板找 console-hc.cloud.tencent.com/_api/apm/… 的请求，「复制为 cURL」再贴一次";
  }
  $("curl-result").textContent = msg;
}

function parseCookieIds(cookieStr) {
  const out = {};
  for (const pair of cookieStr.split(";")) {
    const i = pair.indexOf("=");
    if (i < 0) continue;
    const k = pair.slice(0, i).trim();
    const v = pair.slice(i + 1).trim();
    if (k === "uin") out.uin = v.replace(/^[oO]/, "");
    if (k === "ownerUin") out.ownerUin = v.replace(/^[oO]/, "").replace(/[gG]$/, "");
  }
  return out;
}

// ---------- 事件绑定与初始化 ----------
function bindConfigInputs() {
  $("region").onchange = () => { cfg.region = $("region").value; scheduleSave(); };
  $("region-id").oninput = () => { cfg.regionId = parseInt($("region-id").value) || 4; scheduleSave(); };
  $("instance-id").oninput = () => { cfg.instanceId = $("instance-id").value.trim(); scheduleSave(); };
  $("secret-id").oninput = () => { cfg.secretId = $("secret-id").value.trim(); scheduleSave(); };
  $("secret-key").oninput = () => { cfg.secretKey = $("secret-key").value; scheduleSave(); };
  $("uin").oninput = () => { cfg.uin = $("uin").value.trim(); scheduleSave(); };
  $("owner-uin").oninput = () => { cfg.ownerUin = $("owner-uin").value.trim(); scheduleSave(); };
  $("csrf-code").oninput = () => { cfg.csrfCode = $("csrf-code").value.trim(); scheduleSave(); };
  $("cookie").oninput = () => { cfg.cookie = $("cookie").value; scheduleSave(); };
  for (const radio of document.querySelectorAll('input[name="auth"]')) {
    radio.onchange = () => {
      if (!radio.checked) return;
      cfg.authMode = radio.value;
      $("auth-secret").classList.toggle("hidden", radio.value !== "secret");
      $("auth-cookie").classList.toggle("hidden", radio.value !== "cookie");
      scheduleSave();
    };
  }
  for (const chip of $("quick-ranges").querySelectorAll(".chip")) {
    chip.onclick = () => {
      cfg.useCustom = false;
      cfg.quickRange = chip.dataset.key;
      renderTimeUI();
      scheduleSave();
    };
  }
  $("custom-start").onchange = () => { cfg.customStart = $("custom-start").value; cfg.useCustom = true; renderTimeUI(); scheduleSave(); };
  $("custom-end").onchange = () => { cfg.customEnd = $("custom-end").value; cfg.useCustom = true; renderTimeUI(); scheduleSave(); };
  $("app-search").oninput = () => renderApps(false);
  $("btn-load-apps").onclick = () => loadSources([activeListTab]);
  $("btn-app-all").onclick = () => {
    const t = activeListTab;
    const kw = $("app-search").value.trim().toLowerCase();
    const list = selectedListOf(t);
    for (const a of (sourcesData[t] || []).filter((x) => (x.name + (x.label || "")).toLowerCase().includes(kw))) {
      if (!list.includes(a.name)) list.push(a.name);
    }
    if (t !== "apm") {
      cfg.selectedDbInstances = cfg.selectedDbInstances || {};
      cfg.selectedDbInstances[t] = list;
    }
    renderApps(false);
    renderListTabs();
    scheduleSave();
  };
  $("btn-app-none").onclick = () => {
    if (activeListTab === "apm") cfg.selectedApps = [];
    else {
      cfg.selectedDbInstances = cfg.selectedDbInstances || {};
      cfg.selectedDbInstances[activeListTab] = [];
    }
    renderApps(false);
    renderListTabs();
    scheduleSave();
  };
  $("btn-app-filter-all").onclick = () => setAppFilter(false);
  $("btn-app-selected").onclick = () => setAppFilter(true);
  $("btn-refresh-metrics").onclick = refreshMetrics;
  $("btn-metrics-default").onclick = () => {
    cfg.selectedMetrics = cfg.selectedMetrics.filter((m) => !isApmView(m.view));
    cfg.selectedMetrics.push(...DEFAULT_METRICS.map((d) => ({ ...d })));
    renderMetrics();
    scheduleSave();
  };
  $("btn-query").onclick = query;
  $("btn-query-2").onclick = query;
  $("btn-copy-all").onclick = () => {
    const all = [...document.querySelectorAll(".result-block > div")].map((d) => d.dataset.md).join("\n\n");
    if (all) { copyText(all, null); showOk("已复制全部"); }
  };
  $("btn-parse-curl").onclick = parseCurlFill;
  $("btn-scenario-save").onclick = saveScenario;
  $("btn-scenario-delete").onclick = deleteScenario;
  $("btn-validate-cookie").onclick = async () => {
    $("cookie-result").textContent = "校验中…";
    try {
      $("cookie-result").textContent = await invoke("validate_cookie", { config: cfg });
    } catch (e) {
      $("cookie-result").textContent = String(e);
    }
  };
}

function renderTimeUI() {
  for (const chip of $("quick-ranges").querySelectorAll(".chip")) {
    chip.classList.toggle("active", !cfg.useCustom && chip.dataset.key === cfg.quickRange);
  }
  $("custom-range").classList.toggle("hidden", !cfg.useCustom);
  updateRangeHint();
}

function updateSettingsHint() {
  const ds = (cfg.enabledSources || []).map((t) => SOURCE_NAMES[t] || t).join(" + ");
  const auth = cfg.authMode === "cookie" ? "Cookie 模式" : "密钥模式";
  $("settings-hint").textContent = `已启用：${ds}（${auth}）`;
}

async function init() {
  for (const [val, label] of REGIONS) {
    const opt = document.createElement("option");
    opt.value = val;
    opt.textContent = label;
    $("region").appendChild(opt);
  }
  for (const [key, label] of QUICK_RANGES) {
    const chip = document.createElement("button");
    chip.className = "chip";
    chip.dataset.key = key;
    chip.textContent = label;
    $("quick-ranges").appendChild(chip);
  }

  cfg = await invoke("load_config");
  if (!cfg.selectedMetrics.length) cfg.selectedMetrics = DEFAULT_METRICS.map((d) => ({ ...d }));
  if (!cfg.metricCache.length) cfg.metricCache = DEFAULT_METRICS.map((d) => ({ ...d }));
  if (!Array.isArray(cfg.enabledSources)) cfg.enabledSources = [];
  cfg.enabledSources = cfg.enabledSources.filter((t) => SOURCE_ORDER.includes(t));
  if (!cfg.enabledSources.length) cfg.enabledSources = ["container"];
  if (!cfg.scenarios) cfg.scenarios = [];
  if (!cfg.activeScenario) cfg.activeScenario = "";
  // 配置迁移：补充新增 APM 指标并统一顺序
  for (const d of DEFAULT_METRICS) {
    if (!cfg.metricCache.some((m) => m.name === d.name)) cfg.metricCache.push({ ...d });
    if (!cfg.selectedMetrics.some((m) => m.name === d.name && m.view === d.view)) cfg.selectedMetrics.push({ ...d });
  }
  // 扩展视图指标仅进缓存（默认不勾选，指标面板自行勾选）
  for (const d of APM_VIEW_METRICS) {
    if (!cfg.metricCache.some((m) => m.name === d.name && m.view === d.view)) cfg.metricCache.push({ ...d });
  }
  if (!Array.isArray(cfg.selectedDeployments)) cfg.selectedDeployments = [];
  if (cfg.containerCluster === undefined) cfg.containerCluster = "";
  if (cfg.containerNamespace === undefined) cfg.containerNamespace = "";
  if (isEnabled("container") && !cfg.selectedMetrics.some((m) => m.view === "container")) {
    cfg.selectedMetrics.push(...CONTAINER_METRICS.map((d) => ({ ...d, view: "container" })));
  }
  // 补充已启用数据源默认勾选的核心数据库指标（新增指标不再自动全选，由用户自行勾选）
  for (const t of ["mysql", "redis", "mongodb"]) {
    if (!isEnabled(t)) continue;
    for (const name of DB_DEFAULT_SELECTED[t] || []) {
      const def = (DB_METRICS[t] || []).find((d) => d.name === name);
      if (def && !cfg.selectedMetrics.some((m) => m.name === name && m.view === t)) {
        cfg.selectedMetrics.push({ ...def, view: t });
      }
    }
  }
  const order = new Map(DEFAULT_METRICS.map((d, i) => [d.name + "|" + d.view, i]));
  const sortByDefault = (arr) =>
    [...new Map(arr.map((m) => [m.name + "|" + m.view, m])).values()].sort(
      (a, b) => (order.has(a.name + "|" + a.view) ? order.get(a.name + "|" + a.view) : 99) -
                (order.has(b.name + "|" + b.view) ? order.get(b.name + "|" + b.view) : 99)
    );
  cfg.selectedMetrics = sortByDefault(cfg.selectedMetrics);
  cfg.metricCache = sortByDefault(cfg.metricCache);

  $("region").value = cfg.region || "ap-shanghai";
  $("region-id").value = cfg.regionId ?? 4;
  $("instance-id").value = cfg.instanceId || "";
  $("secret-id").value = cfg.secretId || "";
  $("secret-key").value = cfg.secretKey || "";
  $("cookie").value = cfg.cookie || "";
  $("uin").value = cfg.uin || "";
  $("owner-uin").value = cfg.ownerUin || "";
  $("csrf-code").value = cfg.csrfCode || "";
  document.querySelector(`input[name="auth"][value="${cfg.authMode === "cookie" ? "cookie" : "secret"}"]`).checked = true;
  $("auth-secret").classList.toggle("hidden", cfg.authMode === "cookie");
  $("auth-cookie").classList.toggle("hidden", cfg.authMode !== "cookie");
  if (cfg.useCustom && cfg.customStart) $("custom-start").value = cfg.customStart;
  if (cfg.useCustom && cfg.customEnd) $("custom-end").value = cfg.customEnd;
  $("settings").open = !(cfg.cookie || cfg.secretId);
  $("field-team").classList.remove("hidden");
  updateSettingsHint();

  bindConfigInputs();
  renderScenarioChips();
  renderSourceChips();
  renderTimeUI();
  activeListTab = cfg.enabledSources.find(isEnabled) || "container";
  activeMetricTab = activeListTab;
  renderTabs();
  renderApps(false);
  renderMetrics();

  // 入场动画
  animate(".anim-enter", {
    translateY: [18, 0],
    opacity: [0, 1],
    delay: stagger(90),
    duration: 550,
    ease: "outExpo",
  });

  // 配置齐全时自动加载所有已启用数据源的列表
  if (cfg.secretId || cfg.cookie) {
    loadSources(cfg.enabledSources.filter(isEnabled));
  }
}

init().catch(showErr);
