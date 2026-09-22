import io

# ---------- index.html：停止按钮 + 日志面板 ----------
p = "ui/index.html"
s = io.open(p, encoding="utf-8").read()
if "btn-stop" not in s:
    s = s.replace('''    <button id="btn-query" class="btn-primary btn-big">
      <span class="btn-label">开始统计</span>
    </button>''','''    <button id="btn-query" class="btn-primary btn-big">
      <span class="btn-label">开始统计</span>
    </button>
    <button id="btn-stop" class="btn-ghost hidden">停止</button>''', 1)
    s = s.replace('''  <script src="vendor/anime.umd.min.js"></script>''','''  <details id="log-panel" class="card anim-enter">
    <summary>
      <span class="card-title">实时日志</span>
      <button class="btn-mini" id="btn-log-clear">清空</button>
    </summary>
    <pre id="log-box"></pre>
  </details>

  <script src="vendor/anime.umd.min.js"></script>''', 1)
    io.open(p, "w", encoding="utf-8").write(s)
    print("html ok")

# ---------- style.css：日志样式 ----------
p = "ui/style.css"
s = io.open(p, encoding="utf-8").read()
if "#log-box" not in s:
    s += """
/* ---------- 实时日志 ---------- */
#log-panel { margin-top: 14px; }
#log-panel summary { display: flex; align-items: center; gap: 10px; cursor: pointer; list-style: none; }
#log-panel summary::-webkit-details-marker { display: none; }
#log-box {
  margin: 8px 0 0;
  padding: 8px 10px;
  max-height: 220px;
  overflow-y: auto;
  background: #0f172a;
  color: #cbd5e1;
  border-radius: 8px;
  font-family: Consolas, "Cascadia Mono", monospace;
  font-size: 12px;
  line-height: 1.6;
  white-space: pre-wrap;
}
#log-box .t { color: #64748b; }
#log-box .err { color: #fca5a5; }
#log-box .ok { color: #86efac; }
"""
    io.open(p, "w", encoding="utf-8").write(s)
    print("css ok")

# ---------- app.js：日志与停止逻辑 ----------
p = "ui/app.js"
s = io.open(p, encoding="utf-8").read()

# 日志工具
if "function logLine" not in s:
    s = s.replace('''// ---------- 时间 ----------''','''// ---------- 实时日志 ----------
let logCount = 0;
function logLine(msg, kind) {
  const box = $("log-box");
  if (!box) return;
  const now = new Date();
  const t = `${String(now.getHours()).padStart(2, "0")}:${String(now.getMinutes()).padStart(2, "0")}:${String(now.getSeconds()).padStart(2, "0")}`;
  const line = document.createElement("div");
  if (kind) line.className = kind;
  line.innerHTML = `<span class="t">[${t}]</span> ${String(msg).replace(/</g, "&lt;")}`;
  box.appendChild(line);
  logCount += 1;
  while (box.childElementCount > 400) box.removeChild(box.firstChild);
  box.scrollTop = box.scrollHeight;
}

// ---------- 时间 ----------''', 1)

# 查询状态：耗时计时 + 停止按钮
s = s.replace('''async function query() {
  const r = resolveRange();''','''let queryTimer = null;
let queryToken = 0;

function startQueryUi() {
  queryToken += 1;
  const startedAt = Date.now();
  $("btn-stop").classList.remove("hidden");
  if (queryTimer) clearInterval(queryTimer);
  queryTimer = setInterval(() => {
    const s = Math.round((Date.now() - startedAt) / 1000);
    $("status").classList.remove("ok");
    $("status").textContent = `统计中… 已用时 ${s}s（可点「停止」中断）`;
  }, 1000);
  logLine("开始统计…", "ok");
}

function endQueryUi() {
  if (queryTimer) {
    clearInterval(queryTimer);
    queryTimer = null;
  }
  $("btn-stop").classList.add("hidden");
}

async function query() {
  const r = resolveRange();''', 1)

s = s.replace('''  for (const id of ["btn-query", "btn-query-2"]) {
    const b = $(id);
    b.disabled = true;
    b.classList.add("loading");
  }
  $("status").textContent = "查询中…";
  try {''','''  for (const id of ["btn-query", "btn-query-2"]) {
    const b = $(id);
    b.disabled = true;
    b.classList.add("loading");
  }
  startQueryUi();
  const myToken = queryToken;
  try {''', 1)

s = s.replace('''    const settled = await Promise.allSettled(tasks);
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
}''','''    const settled = await Promise.allSettled(tasks);
    if (myToken !== queryToken) {
      logLine("本次查询已停止，忽略返回结果");
      return;
    }
    const groups = [];
    for (const s of settled) {
      if (s.status === "fulfilled") groups.push(s.value);
      else {
        const msg = String(s.reason);
        logLine(`查询出错：${msg}`, "err");
        groups.push({ src: "error", results: [], error: msg });
      }
    }
    renderResults(groups, r);
    const total = groups.reduce((n, g) => n + g.results.length, 0);
    logLine(`统计完成：${total} 个对象`, "ok");
    showOk(`统计完成：${total} 个对象`);
  } catch (e) {
    logLine(`统计失败：${e}`, "err");
    showErr(e);
  } finally {
    endQueryUi();
    for (const id of ["btn-query", "btn-query-2"]) {
      const b = $(id);
      b.disabled = false;
      b.classList.remove("loading");
    }
  }
}''', 1)

# 停止按钮 + 日志清空 + 事件监听
s = s.replace('''  $("btn-query").onclick = query;''','''  $("btn-stop").onclick = () => {
    queryToken += 1; // 让返回结果失效
    logLine("已请求停止（后台正在收尾）…", "err");
    invoke("cancel_query").catch(() => {});
    endQueryUi();
    for (const id of ["btn-query", "btn-query-2"]) {
      const b = $(id);
      b.disabled = false;
      b.classList.remove("loading");
    }
    $("status").classList.remove("ok");
    $("status").textContent = "已停止";
  };
  $("btn-log-clear").onclick = () => { $("log-box").innerHTML = ""; logCount = 0; };
  $("btn-query").onclick = query;''', 1)

s = s.replace('''// 监听后端「会话刷新中」事件
try {
  window.__TAURI__.event.listen("session-refresh", (e) => {
    $("status").classList.remove("ok");
    $("status").textContent = String(e.payload || "");
  });
} catch (_) {}''','''// 监听后端事件：进度日志 / 会话刷新
try {
  window.__TAURI__.event.listen("query-log", (e) => logLine(e.payload));
  window.__TAURI__.event.listen("session-refresh", (e) => {
    logLine(String(e.payload || ""), "err");
    $("status").classList.remove("ok");
    $("status").textContent = String(e.payload || "");
  });
} catch (_) {}''', 1)
io.open(p, "w", encoding="utf-8").write(s)
print("app.js ok")
