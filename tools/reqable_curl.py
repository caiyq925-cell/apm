"""取 id=99 原始记录结构，定位完整 URL/查询参数。"""
import json, subprocess, time

EXE = r"D:\Program Files\Reqable\mcp-server.exe"
proc = subprocess.Popen([EXE, "--host", "127.0.0.1", "--port", "9000", "--scope", "minimal"],
    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding="utf-8", bufsize=1)
def send(o): proc.stdin.write(json.dumps(o, ensure_ascii=False) + "\n"); proc.stdin.flush()
def read_msg(timeout=20):
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
_mid=[0]
def tool(name, args, timeout=20):
    _mid[0]+=1
    send({"jsonrpc":"2.0","id":_mid[0],"method":"tools/call","params":{"name":name,"arguments":args}})
    r = read_msg(timeout)
    if not r: return None
    if "error" in r: return {"__err__": r["error"].get("message","")[:200]}
    texts=[c.get("text","") for c in r.get("result",{}).get("content",[]) if c.get("type")=="text"]
    out="\n".join(texts)
    try: return json.loads(out)
    except Exception: return {"__text__": out}

send({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"p","version":"1"}}})
read_msg(); send({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}); time.sleep(0.3)

# 也可用 generate_curl 直接拿完整 curl
cur = tool("capture_live_generate_curl", {"id": 99}, timeout=25)
print("=== cURL（前 1200 字符） ===")
text = cur if isinstance(cur, str) else (cur.get("curl") if isinstance(cur, dict) else "")
if isinstance(cur, dict) and "__text__" in cur: text = cur["__text__"]
print(str(text)[:1200])

d = tool("capture_live_get_by_id", {"id": 99})
if isinstance(d, dict):
    print()
    print("=== record 顶层键 ===", list(d.keys()))
    req = d.get("request") or {}
    print("=== request 键 ===", list(req.keys()))
    for k, v in req.items():
        if k in ("headers","body","cookies"): continue
        print(f"   {k} = {str(v)[:300]}")
proc.terminate()
