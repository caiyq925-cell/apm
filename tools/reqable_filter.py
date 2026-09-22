"""通过 Reqable MCP 筛选并查看 APM 相关抓包请求。"""
import json, subprocess, sys, time

EXE = r"D:\Program Files\Reqable\mcp-server.exe"
proc = subprocess.Popen(
    [EXE, "--host", "127.0.0.1", "--port", "9000", "--scope", "minimal"],
    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    text=True, encoding="utf-8", bufsize=1,
)

def send(o):
    proc.stdin.write(json.dumps(o, ensure_ascii=False) + "\n"); proc.stdin.flush()

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

# 先看 filter 工具的 schema
r = call("tools/list", {}, 2)
for t in (r or {}).get("result", {}).get("tools", []):
    if t["name"] == "capture_live_filter":
        print("=== capture_live_filter schema ===")
        print(json.dumps(t.get("inputSchema"), ensure_ascii=False, indent=1)[:1500])
        break

print()
print("=== 尝试过滤 APM 请求 ===")
for label, filters in [
    ("url 含 _api/apm", {"filters": {"url": {"contain": "_api/apm"}}}),
    ("url 含 _api/apm (数组)", {"filters": [{"key": "url", "op": "contain", "value": "_api/apm"}]}),
]:
    r = call("tools/call", {"name": "capture_live_filter", "arguments": filters}, 10)
    print(f"--- {label}")
    print(json.dumps(r, ensure_ascii=False)[:800] if r else "无响应")
    print()
