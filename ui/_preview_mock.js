// 预览页专用：桩掉 Tauri 后端，让 app.js 在纯浏览器里可跑（不改动任何生产文件）
const NOW = Math.floor(Date.now() / 1000);
const CONTAINER_METRICS_MOCK = [
  { name: "cpu_util_limit", cn: "CPU最大利用率", unit: "%", view: "container" },
  { name: "mem_util_limit", cn: "内存最大利用率", unit: "%", view: "container" },
  { name: "pod_ready", cn: "Pod数", unit: "pod", view: "container" },
  { name: "cpu_limit_cores", cn: "CPU limit", unit: "核", view: "container" },
  { name: "mem_limit_mib", cn: "内存 limit", unit: "MiB", view: "container" },
];
const APM_METRICS_MOCK = [
  { name: "request_count", view: "service_metric", cn: "请求量" },
  { name: "qps_avg", view: "computed", cn: "平均(次/秒)" },
  { name: "error_request_count", view: "service_metric", cn: "异常请求量" },
  { name: "error_req_rate_avg", view: "service_metric", cn: "错误率" },
  { name: "duration_avg", view: "service_metric", cn: "平均耗时" },
  { name: "duration_max", view: "service_metric", cn: "最大耗时" },
];
const MYSQL_MOCK = ["cpu", "mem", "qps", "conn", "conn_limit", "disk", "slow"].map((n) => ({
  name: n, view: "mysql",
  cn: { cpu: "CPU最高点", mem: "内存最高点", qps: "峰值QPS", conn: "最高连接数", conn_limit: "连接数上限", disk: "磁盘使用率", slow: "慢查询数" }[n],
  unit: { cpu: "%", mem: "%", qps: "次/秒", conn: "个", conn_limit: "个", disk: "%", slow: "次" }[n],
  noPrefix: n === "disk",
}));
const DEPLOYS = Array.from({ length: 24 }, (_, i) => ({
  name: `order-svc-${String(i + 1).padStart(2, "0")}`,
  apmName: `order-svc-${String(i + 1).padStart(2, "0")}`,
}));

const MOCK_CFG = {
  region: "ap-shanghai", regionId: 4, instanceId: "apm-tHxaVZjHH",
  sessionRefreshMinutes: 10, sessionRefreshBackoffMinutes: 2, sessionRefreshMaxFails: 3, sessionAutoRefresh: true,
  useCustom: false, quickRange: "1h", customStart: "", customEnd: "",
  enabledSources: ["container", "mysql"],
  selectedApps: [],
  selectedDbInstances: { mysql: ["cdb-1001", "cdb-1002"] },
  selectedDeployments: DEPLOYS.map((d) => d.name),
  selectedMetrics: [...CONTAINER_METRICS_MOCK, ...APM_METRICS_MOCK, ...MYSQL_MOCK.filter((m) => m.name !== "conn_limit" && m.name !== "slow")],
  metricCache: [...APM_METRICS_MOCK],
  scenarios: [{ name: "日常巡检", sources: ["container", "mysql"], apps: [], dbInstances: { mysql: ["cdb-1001"] }, metrics: [...CONTAINER_METRICS_MOCK, ...APM_METRICS_MOCK], deployments: DEPLOYS.slice(0, 6).map((d) => d.name) }],
  activeScenario: "",
  containerCluster: "cls-7xka2p1q", containerNamespace: "online",
};

const delay = (ms) => new Promise((r) => setTimeout(r, ms));

function containerResults() {
  return DEPLOYS.map((d, i) => ({
    name: d.name,
    error: i === 7 ? "旧网关返回 1216：会话失效" : null,
    values: [
      { name: "cpu_util_limit", value: 12 + i * 1.3 },
      { name: "mem_util_limit", value: 45 + (i % 5) * 6 },
      { name: "pod_ready", value: 4 },
      { name: "cpu_limit_cores", value: 2 },
      { name: "mem_limit_mib", value: 4096 },
      { name: "request_count", value: 152300 + i * 977 },
      { name: "error_request_count", value: i === 3 ? 42 : 0 },
      { name: "error_req_rate_avg", value: i === 3 ? 1.26 : 0 },
      { name: "duration_avg", value: 23.5 + i },
      { name: "duration_max", value: 812 + i * 13 },
    ],
  }));
}
function mysqlResults() {
  return ["cdb-1001", "cdb-1002"].map((id, i) => ({
    name: id,
    error: null,
    values: [
      { name: "cpu", value: 31.2 + i },
      { name: "mem", value: 68.4 },
      { name: "qps", value: 4210 },
      { name: "conn", value: 820 },
      { name: "conn_limit", value: 2000 },
      { name: "disk", value: 57.1 },
    ],
  }));
}

window.__TAURI__ = {
  core: {
    invoke: async (cmd, args) => {
      switch (cmd) {
        case "load_config": return JSON.parse(JSON.stringify(MOCK_CFG));
        case "session_status": return { ready: true, uin: "1000321658", fetchedAt: NOW - 420, ageSecs: 420 };
        case "list_clusters": return [{ id: "cls-7xka2p1q", name: "prod-shanghai" }, { id: "cls-9mnb3c4d", name: "staging" }];
        case "list_namespaces": return ["default", "online", "infra"];
        case "list_deployments": return DEPLOYS.map((d) => ({ name: d.name, apmName: d.apmName }));
        case "list_db_instances": return Array.from({ length: 8 }, (_, i) => ({ id: `cdb-${1001 + i}`, name: `订单库-${i + 1}` }));
        case "query_container_metrics": await delay(900); return containerResults();
        case "query_db_metrics": await delay(1600); return mysqlResults();
        case "save_config": case "log_ui": case "cancel_query": case "notify_session_invalid": return null;
        default: return null;
      }
    },
  },
  event: { listen: () => Promise.resolve(() => {}) },
};
