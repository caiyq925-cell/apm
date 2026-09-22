"""拉取全部抓包记录，按 URL 归类，寻找 APM 实例分析接口。"""
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
        return {"__err__": r["error"].get("message","")[:150]}
    texts = [c.get("text","") for c in r.get("result",{}).get("content",[]) if c.get("type")=="text"]
    out = "\n".join(texts)
    try: return json.loads(out)
    except Exception: return {"__text__": out}

send({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"probe","version":"1"}}})
read_msg(); send({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}); time.sleep(0.3)

allids = tool("capture_live_filter", {"filters": []})
ids = []
if isinstance(allids, list):
    ids = [x.get("id") if isinstance(x, dict) else x for x in allids]
elif isinstance(allids, dict):
    for k in ("ids","recordIds","records","items","result"):
        if isinstance(allids.get(k), list):
            ids = [x.get("id") if isinstance(x, dict) else x for x in allids[k]]
            break
    if not ids and "__text__" in allids:
        print("filter 返回文本:", allids["__text__"][:200], flush=True)
print(f"总记录数: {len(ids)}", flush=True)

rows = []
for i in ids:
    if not isinstance(i, int): continue
    d = tool("capture_live_get_by_id", {"id": i}, timeout=15)
    if not isinstance(d, dict) or "__err__" in d:
        continue
    req = d.get("request") or {}
    url = req.get("url") or d.get("url") or ""
    body = req.get("body") or ""
    if isinstance(body, dict): body = json.dumps(body, ensure_ascii=False)
    cmd = ""
    try: cmd = (json.loads(body) or {}).get("cmd","")
    except Exception: pass
    rows.append((i, cmd, url, d))

print(f"取到详情: {len(rows)} 条", flush=True)
print("=== 含 apm 的请求 ===", flush=True)
for i, cmd, url, _ in rows:
    if "apm" in url.lower() or cmd:
        print(f"  id={i:4d} cmd={cmd or '-':32s} {url[:105]}", flush=True)
print("=== 其他请求（抽样 15 条） ===", flush=True)
for i, cmd, url, _ in rows[:15]:
    if not ("apm" in url.lower() or cmd):
        print(f"  id={i:4d} {url[:120]}", flush=True)

# 保存完整数据供后续分析
with open("tools/capture_dump.json","w",encoding="utf-8") as f:
    json.dump([{"id":i,"cmd":cmd,"url":url,"raw":d} for i,cmd,url,d in rows], f, ensure_ascii=False, indent=1)
print("已保存 tools/capture_dump.json", flush=True)
proc.terminate()
