"""查找抓包中的 TKE / 容器相关请求。"""
import json, subprocess, time

EXE = r"D:\Program Files\Reqable\mcp-server.exe"
proc = subprocess.Popen([EXE, "--host","127.0.0.1","--port","9000","--scope","minimal"],
    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding="utf-8", bufsize=1)
def send(o): proc.stdin.write(json.dumps(o, ensure_ascii=False)+"\n"); proc.stdin.flush()
def read_msg(t=25):
    end=time.time()+t
    while time.time()<end:
        l=proc.stdout.readline()
        if not l: return None
        l=l.strip()
        if not l: continue
        try: m=json.loads(l)
        except: continue
        if "id" in m: return m
    return None
_mid=[0]
def tool(name, args, timeout=25):
    _mid[0]+=1
    send({"jsonrpc":"2.0","id":_mid[0],"method":"tools/call","params":{"name":name,"arguments":args}})
    r=read_msg(timeout)
    if not r: return None
    if "error" in r: return {"__err__": r["error"].get("message","")[:150]}
    texts=[c.get("text","") for c in r.get("result",{}).get("content",[]) if c.get("type")=="text"]
    out="\n".join(texts)
    try: return json.loads(out)
    except: return {"__text__": out}

send({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"p","version":"1"}}})
read_msg(); send({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}); time.sleep(0.3)
tool("capture_live_set_enabled", {"enabled": True})

ids = tool("capture_live_filter", {"filters": []})
if isinstance(ids, dict): ids = ids.get("ids") or ids.get("recordIds") or []
if ids and isinstance(ids[0], dict): ids = [x.get("id") for x in ids]
print(f"抓包记录总数: {len(ids or [])}")

rows=[]
for i in (ids or [])[-80:]:
    d = tool("capture_live_get_by_id", {"id": i}, timeout=15)
    if not isinstance(d, dict) or "__err__" in d: continue
    req = d.get("request") or {}
    path = req.get("path") or ""
    if any(k in path.lower() for k in ["tke","cluster","k8s","pod","workload","namespace","monitor"]):
        body = req.get("body") or {}
        inner = body.get("text") if isinstance(body, dict) else None
        cmd = ""
        try: cmd = json.loads(inner).get("cmd","") if inner else ""
        except Exception: cmd = ""
        rows.append((i, cmd, path[:130]))
print(f"TKE/容器相关: {len(rows)} 条")
for i, cmd, p in rows:
    print(f"  id={i:4d} cmd={cmd or '-':34s} {p}")
proc.terminate()
