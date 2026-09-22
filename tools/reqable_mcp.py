"""用 Reqable 官方 mcp-server.exe 作为 stdio MCP 子进程，列出抓包中的请求。"""
import json, subprocess, sys, time, threading

EXE = r"D:\Program Files\Reqable\mcp-server.exe"

proc = subprocess.Popen(
    [EXE, "--host", "127.0.0.1", "--port", "9000", "--scope", "minimal"],
    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    text=True, encoding="utf-8", bufsize=1,
)

def send(obj):
    proc.stdin.write(json.dumps(obj, ensure_ascii=False) + "\n")
    proc.stdin.flush()

def read_msg(timeout=20):
    """读一行 JSON-RPC 响应（若为 notification 则跳过）"""
    deadline = time.time() + timeout
    while time.time() < deadline:
        line = proc.stdout.readline()
        if not line:
            return None
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            print("[非JSON输出]", line[:200], file=sys.stderr)
            continue
        if "id" in msg:
            return msg
    return None

def call(method, params=None, mid=1):
    send({"jsonrpc": "2.0", "id": mid, "method": method, "params": params or {}})
    return read_msg()

# 1. initialize
r = call("initialize", {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "apm-monitor", "version": "0.1"}})
print("=== initialize ===")
print(json.dumps(r, ensure_ascii=False)[:500] if r else "无响应")
send({"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}})
time.sleep(0.5)

# 2. 工具清单
r = call("tools/list", {}, 2)
print()
print("=== tools/list ===")
if r and "result" in r:
    for t in r["result"].get("tools", []):
        print(f"- {t['name']}: {t.get('description','')[:120]}")
        props = (t.get("inputSchema") or {}).get("properties") or {}
        if props:
            print(f"    args: {list(props.keys())}")
else:
    print(json.dumps(r, ensure_ascii=False)[:800] if r else "无响应")

proc.terminate()
