"""抓取最近控制台请求（TKE/容器相关），导出到临时目录（仓库外，避免凭证误提交）。"""
import json, subprocess, time, os, tempfile

EXE = r"D:\Program Files\Reqable\mcp-server.exe"
OUT = os.path.join(tempfile.gettempdir(), "reqable_tke_dump.json")

proc = subprocess.Popen([EXE,"--host","127.0.0.1","--port","9000","--scope","minimal"],
    stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,encoding="utf-8",bufsize=1)
def send(o): proc.stdin.write(json.dumps(o,ensure_ascii=False)+"\n"); proc.stdin.flush()
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
def tool(name,args,timeout=25):
    _mid[0]+=1
    send({"jsonrpc":"2.0","id":_mid[0],"method":"tools/call","params":{"name":name,"arguments":args}})
    r=read_msg(timeout)
    if not r or "error" in r: return None
    texts=[c.get("text","") for c in r.get("result",{}).get("content",[]) if c.get("type")=="text"]
    try: return json.loads("\n".join(texts))
    except: return None

send({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"p","version":"1"}}})
read_msg(); send({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}); time.sleep(0.3)

ids = tool("capture_live_filter", {"filters": []})
if isinstance(ids, dict): ids = ids.get("ids") or []
if ids and isinstance(ids[0], dict): ids=[x.get("id") for x in ids]
print(f"记录总数 {len(ids)}", flush=True)

interesting=[]
for i in ids:
    d = tool("capture_live_get_by_id", {"id":i}, 15)
    if not isinstance(d, dict): continue
    url = d.get("url") or ""
    req = d.get("request") or {}
    path = req.get("path") or ""
    full = (url or "") + path
    if "console.cloud.tencent.com" not in full and "console-hc.cloud.tencent.com" not in full:
        continue
    body = req.get("body") or {}
    cmd = ""
    if isinstance(body, dict):
        t = body.get("text")
        if t:
            try: cmd = json.loads(t).get("cmd","")
            except Exception: cmd = ""
    else:
        try: cmd = json.loads(body).get("cmd","")
        except Exception: cmd = ""
    interesting.append({"id": i, "url": (url or "")[:200], "path": path[:300], "cmd": cmd, "raw": d})

with open(OUT, "w", encoding="utf-8") as f:
    json.dump(interesting, f, ensure_ascii=False)
print(f"控制台请求 {len(interesting)} 条 -> {OUT}", flush=True)
for it in interesting:
    print(f"  id={it['id']:4d} cmd={it['cmd'] or '-':32s} {it['path'][:110]}", flush=True)
proc.terminate()
