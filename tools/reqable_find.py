"""筛选 APM 请求并查看详情（找实例分析接口）。"""
import json, subprocess, sys, time

EXE = r"D:\Program Files\Reqable\mcp-server.exe"
proc = subprocess.Popen(
    [EXE, "--host", "127.0.0.1", "--port", "9000", "--scope", "minimal"],
    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    text=True, encoding="utf-8", bufsize=1,
)
def send(o): proc.stdin.write(json.dumps(o, ensure_ascii=False) + "\n"); proc.stdin.flush()
def read_msg(timeout=30):
    end = time.time() + timeout
    while time.time() < end:
        line = proc.stdout.readline()
        if not line: return None
        line = line.strip()
        if not line: continue
        try: m = json.loads(line)
        except json.JSONDecodeError: continue
        if "id" in m: return m
    return None
def call(method, params=None, mid=1):
    send({"jsonrpc":"2.0","id":mid,"method":method,"params":params or {}})
    return read_msg()

call("initialize", {"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"probe","version":"1"}})
send({"jsonrpc":"2.0","method":"notifications/initialized","params":{}})
time.sleep(0.3)

def tool(name, args, mid):
    r = call("tools/call", {"name": name, "arguments": args}, mid)
    if not r: return None
    if "error" in r:
        print(f"[{name}] ERROR: {r['error'].get('message','')[:200]}")
        return None
    res = r.get("result", {})
    # MCP 结果通常是 content 数组
    texts = [c.get("text", "") for c in res.get("content", []) if c.get("type") == "text"]
    out = "\n".join(texts)
    try: return json.loads(out)
    except Exception: return out

ids = tool("capture_live_filter", {"filters": [{"type": "keyword", "pattern": "_api/apm"}]}, 10)
print("=== 匹配到 _api/apm 的请求数 ===")
print(json.dumps(ids, ensure_ascii=False)[:600] if not isinstance(ids, list) else f"{len(ids)} 条: {ids[:40]}")

if isinstance(ids, dict):
    ids = ids.get("ids") or ids.get("recordIds") or ids.get("result") or []
if isinstance(ids, list) and ids and isinstance(ids[0], dict):
    ids = [x.get("id") for x in ids]

print()
print("=== 逐条查看 URL / cmd（最近 40 条） ===")
for i in (ids or [])[:40]:
    d = tool("capture_live_get_by_id", {"id": i}, 100 + int(i) if str(i).isdigit() else 100)
    if not isinstance(d, dict):
        continue
    req = d.get("request") or {}
    resp = d.get("response") or {}
    url = req.get("url") or d.get("url") or ""
    body = req.get("body") or ""
    if isinstance(body, dict): body = json.dumps(body, ensure_ascii=False)
    cmd = ""
    try:
        cmd = json.loads(body).get("cmd", "")
    except Exception:
        cmd = ""
    print(f"  id={i} {cmd or url[:110]}")
