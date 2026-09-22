"""列出抓包中的 APM 接口请求，定位「实例分析」数据接口。"""
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
_mid = [0]
def tool(name, args, timeout=20):
    _mid[0] += 1
    send({"jsonrpc":"2.0","id":_mid[0],"method":"tools/call","params":{"name":name,"arguments":args}})
    r = read_msg(timeout)
    if not r: return None
    if "error" in r:
        print(f"[{name}] ERR {r['error'].get('message','')[:150]}", flush=True); return None
    texts = [c.get("text","") for c in r.get("result",{}).get("content",[]) if c.get("type")=="text"]
    out = "\n".join(texts)
    try: return json.loads(out)
    except Exception: return out

send({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"probe","version":"1"}}})
read_msg(); send({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}); time.sleep(0.3)

ids = tool("capture_live_filter", {"filters": [{"type":"keyword","pattern":"_api/apm"}]})
if isinstance(ids, dict): ids = ids.get("ids") or ids.get("recordIds") or []
if ids and isinstance(ids[0], dict): ids = [x.get("id") for x in ids]
print(f"APM 请求 {len(ids or [])} 条: {ids}", flush=True)

for i in (ids or [])[:25]:
    d = tool("capture_live_get_by_id", {"id": i})
    if not isinstance(d, dict):
        print(f"  id={i} 详情获取失败", flush=True); continue
    req = d.get("request") or {}
    url = req.get("url") or ""
    body = req.get("body") or ""
    if isinstance(body, dict): body = json.dumps(body, ensure_ascii=False)
    cmd = ""
    try: cmd = json.loads(body).get("cmd","")
    except Exception: pass
    print(f"  id={i}  cmd={cmd or '-'}  url={url[:100]}", flush=True)

proc.terminate()
