import io
p = "ui/app.js"
s = io.open(p, encoding="utf-8").read()

old = '''  box.appendChild(mk("集群", containerClusters.map((c) => ({ value: c.id, text: `${c.name}（${c.id}）` })), cfg.containerCluster,
    (v) => { cfg.containerCluster = v; cfg.containerNamespace = ""; reloadContainerScope(); }));
  box.appendChild(mk("命名空间", containerNamespaces.map((n) => ({ value: n, text: n })), cfg.containerNamespace,
    (v) => { cfg.containerNamespace = v; reloadContainerScope(); }));
}'''
new = '''  box.appendChild(mk("集群", containerClusters.map((c) => ({ value: c.id, text: `${c.name}（${c.id}）` })), cfg.containerCluster,
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
}'''
assert old in s, "pickers 片段未找到"
s = s.replace(old, new, 1)
io.open(p, "w", encoding="utf-8").write(s)
print("ok")
