import io

# ---------- 1) CSS：自定义下拉样式 ----------
p = "ui/style.css"
s = io.open(p, encoding="utf-8").read()
if ".combo-list" not in s:
    s += """
/* ---------- 可搜索下拉（自定义 combobox） ---------- */
.combo { position: relative; display: inline-block; }
.combo > input { padding-right: 24px; cursor: text; }
.combo::after {
  content: "";
  position: absolute;
  right: 9px; top: 50%;
  width: 0; height: 0;
  margin-top: -1px;
  border-left: 4px solid transparent;
  border-right: 4px solid transparent;
  border-top: 5px solid #7c8aa0;
  pointer-events: none;
}
.combo-list {
  position: absolute;
  top: calc(100% + 4px);
  left: 0; right: 0;
  max-height: 220px;
  overflow-y: auto;
  background: #fff;
  border: 1px solid var(--border);
  border-radius: 8px;
  box-shadow: 0 6px 20px rgba(15, 23, 42, 0.14);
  z-index: 40;
  padding: 4px;
}
.combo-list.hidden { display: none; }
.combo-item {
  padding: 5px 9px;
  border-radius: 6px;
  cursor: pointer;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.combo-item:hover, .combo-item.hover { background: #eef4ff; }
.combo-item.active { color: var(--primary); font-weight: 600; }
.combo-empty { padding: 6px 9px; color: var(--muted); font-size: 12px; }
"""
    io.open(p, "w", encoding="utf-8").write(s)
    print("css ok")

# ---------- 2) JS：可搜索下拉组件 + 命名空间/集群使用 ----------
p = "ui/app.js"
s = io.open(p, encoding="utf-8").read()

combo = '''
// 可搜索下拉：点击展开全部、输入过滤、方向键+回车选择
function createCombo(options, value, placeholder, onSelect) {
  const wrap = document.createElement("div");
  wrap.className = "combo";
  const input = document.createElement("input");
  input.value = value || "";
  input.placeholder = placeholder || "";
  input.style.minWidth = "220px";
  const list = document.createElement("div");
  list.className = "combo-list hidden";

  const renderList = (kw) => {
    list.innerHTML = "";
    const filtered = options.filter((o) => o.toLowerCase().includes((kw || "").toLowerCase()));
    if (!filtered.length) {
      const d = document.createElement("div");
      d.className = "combo-empty";
      d.textContent = "无匹配项";
      list.appendChild(d);
      return;
    }
    for (const o of filtered) {
      const it = document.createElement("div");
      it.className = "combo-item" + (o === value ? " active" : "");
      it.textContent = o;
      it.onmousedown = (e) => {
        e.preventDefault();
        input.value = o;
        hide();
        onSelect(o);
      };
      list.appendChild(it);
    }
  };
  const show = (kw) => {
    list.classList.remove("hidden");
    renderList(kw);
  };
  const hide = () => list.classList.add("hidden");
  wrap.__hide = hide;

  input.onfocus = () => show("");
  input.onclick = () => show("");
  input.oninput = () => show(input.value);
  input.onkeydown = (e) => {
    const items = [...list.querySelectorAll(".combo-item")];
    const cur = items.findIndex((x) => x.classList.contains("hover"));
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (list.classList.contains("hidden")) show(input.value);
      const all = [...list.querySelectorAll(".combo-item")];
      if (!all.length) return;
      const next = e.key === "ArrowDown" ? Math.min(all.length - 1, cur + 1) : Math.max(0, cur - 1);
      all.forEach((x) => x.classList.remove("hover"));
      all[next].classList.add("hover");
      all[next].scrollIntoView({ block: "nearest" });
    } else if (e.key === "Enter") {
      e.preventDefault();
      const target = items.find((x) => x.classList.contains("hover")) || items[0];
      if (target) {
        input.value = target.textContent;
        hide();
        onSelect(target.textContent);
      }
    } else if (e.key === "Escape") {
      hide();
    }
  };
  wrap.appendChild(input);
  wrap.appendChild(list);
  return wrap;
}

// 点击空白处收起所有下拉
document.addEventListener("mousedown", (e) => {
  document.querySelectorAll(".combo").forEach((c) => {
    if (!c.contains(e.target) && c.__hide) c.__hide();
  });
});

function renderContainerPickers() {'''
s = s.replace("function renderContainerPickers() {", combo, 1)

# 命名空间：datalist → combo
old_ns = '''  // 命名空间：可输入搜索的下拉（datalist）
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
}'''
new_ns = '''  // 命名空间：可搜索下拉
  const nsLabel = document.createElement("label");
  nsLabel.className = "field";
  nsLabel.textContent = "命名空间";
  nsLabel.appendChild(
    createCombo(containerNamespaces, cfg.containerNamespace, "输入关键字筛选…", (v) => {
      if (v === cfg.containerNamespace) return;
      cfg.containerNamespace = v;
      reloadContainerScope();
    })
  );
  box.appendChild(nsLabel);
}'''
assert old_ns in s, "命名空间片段未找到"
s = s.replace(old_ns, new_ns, 1)

# 集群也用同样的可搜索下拉
old_cl = '''  box.appendChild(mk("集群", containerClusters.map((c) => ({ value: c.id, text: `${c.name}（${c.id}）` })), cfg.containerCluster,
    (v) => { cfg.containerCluster = v; cfg.containerNamespace = ""; reloadContainerScope(); }));'''
new_cl = '''  const clusterLabel = document.createElement("label");
  clusterLabel.className = "field";
  clusterLabel.textContent = "集群";
  const clusterText = (id) => {
    const c = containerClusters.find((x) => x.id === id);
    return c ? `${c.name}（${c.id}）` : id;
  };
  clusterLabel.appendChild(
    createCombo(
      containerClusters.map((c) => `${c.name}（${c.id}）`),
      cfg.containerCluster ? clusterText(cfg.containerCluster) : "",
      "输入关键字筛选…",
      (text) => {
        const hit = containerClusters.find((c) => `${c.name}（${c.id}）` === text);
        if (!hit || hit.id === cfg.containerCluster) return;
        cfg.containerCluster = hit.id;
        cfg.containerNamespace = "";
        reloadContainerScope();
      }
    )
  );
  box.appendChild(clusterLabel);'''
assert old_cl in s, "集群片段未找到"
s = s.replace(old_cl, new_cl, 1)
io.open(p, "w", encoding="utf-8").write(s)
print("js ok")
