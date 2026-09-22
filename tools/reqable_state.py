"""检查 Reqable 抓包状态：开启 live capture，统计记录数。"""
import json, subprocess, sys, time

EXE = r"D:\Program Files\Reqable\mcp-server.exe"
proc = subprocess.Popen(
    [EXE, "--host", "127.0.0.1", "--port", "9000", "--scope", "minimal"],
    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    text=True, encoding="utf-8", bufsize=1,
)
def send(o):
    proc.stdin.write(json.dumps(o, ensure_ascii=False) + "\n"); proc.stdin.flush()
def read_msg(timeout=15):
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

def tool(name, args, mid, timeout=15):
    r = call("tools/call", {"name": name, "arguments": args}, mid) if False else None
    send({"jsonrpc":"2.0","id":mid,"method":"tools/call","params":{"name":name,"arguments":args}})
    r = read_msg(timeout)
    if not r: 
        print(f"[{name}] 超时无响应", flush=True); return None
    if "error" in r:
        print(f"[{name}] ERROR: {r['error'].get('message','')[:200]}", flush=True); return None
    res = r.get("result", {})
    texts = [c.get("text","") for c in res.get("content",[]) if c.get("type")=="text"]
    out = "\n".join(texts)
    try: return json.loads(out)
    except Exception: return out

call("initialize", {"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"probe","version":"1"}})
send({"jsonrpc":"2.0","method":"notifications/initialized","params":{}})
time.sleep(0.3)

print("1) 开启 live capture …", flush=True)
print("   ->", json.dumps(tool("capture_live_set_enabled", {"enabled": True}, 5), ensure_ascii=False)[:200], flush=True)

print("2) 列出全部记录（空过滤）…", flush=True)
allids = tool("capture_live_filter", {"filters": []}, 10, timeout=25)
if isinstance(allids, dict):
    allids = allids.get("ids") or allids.get("recordIds") or []
if isinstance(allids, list) and allids and isinstance(allids[0], dict):
    allids = [x.get("id") for x in allids]
print(f"   记录数: {len(allids) if isinstance(allids, list) else '未知'}", flush=True)
print("   样例:", json.dumps(allids[:15] if isinstance(allids, list) else allids, ensure_ascii=False)[:300], flush=True)

proc.terminate()
