# -*- coding: utf-8 -*-
# Fetch the qrconnect page and its JS bundle; search for the local WeChat
# detection endpoint (127.0.0.1 / localhost / port / quick-login keywords).
import re
import urllib.request

UA = {"User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64)"}
TOOLS = r"D:\Users\ai_apm\apm-monitor\tools"

def get(url):
    req = urllib.request.Request(url, headers=UA)
    return urllib.request.urlopen(req, timeout=20).read().decode("utf-8", "replace")

def main():
    url = ("https://open.weixin.qq.com/connect/qrconnect?appid=wxca396cd1df083b9d"
           "&redirect_uri=https%3A%2F%2Fcloud.tencent.com%2Flogin%2Fcallback"
           "&response_type=code&scope=snsapi_login")
    html = get(url)
    with open(TOOLS + r"\_qrpage.html", "w", encoding="utf-8") as f:
        f.write(html)
    print("html len", len(html))
    scripts = re.findall(r"<script[^>]+src=[\"']([^\"']+)", html)
    for s in scripts:
        print("SCRIPT:", s)
    # inline JS may contain the detection logic too
    for kw in ["127.0.0.1", "localhost", "14013", "quick", "fast", "local"]:
        if kw in html:
            idx = html.find(kw)
            print("HTML has", kw, ":", html[max(0, idx - 60):idx + 80].replace("\n", " "))
    # fetch each external JS and grep
    for s in scripts:
        full = s if s.startswith("http") else ("https://open.weixin.qq.com" + s)
        try:
            js = get(full)
        except Exception as e:
            print("fetch fail", full, e)
            continue
        name = full.split("/")[-1].split("?")[0]
        with open(TOOLS + "\\_qr_" + name, "w", encoding="utf-8") as f:
            f.write(js)
        hits = []
        for kw in ["127.0.0.1", "localhost", "local", "14013", "14016", "quick", "fast", "detect"]:
            for m in re.finditer(re.escape(kw), js):
                hits.append((kw, m.start()))
        print(name, "len", len(js), "kw hits:", len(hits))
        for kw, pos in hits[:12]:
            print("   ", kw, "->", js[max(0, pos - 50):pos + 70].replace("\n", " "))

main()
