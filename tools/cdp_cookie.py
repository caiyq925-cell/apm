"""直连登录窗口的 CDP，把扫出来的 Cookie 抓下来，再隔一段时间反复试探是否变可用。

用途：判断"登录窗口的会话被控制台判为登录态验证失败"是即时失效，还是登录后需要传播时间。

用法（先在界面上点开「扫码登录」，让登录窗口存在）：
    python tools/cdp_cookie.py
"""
import datetime
import json
import os
import sys
import time
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import _cdp
import _session

PORT = _cdp.DEFAULT_PORT


def grab_cookie():
    """抓当前登录窗口页面的 Cookie，按 build_cookie 的口径扁平化。"""
    page = _cdp.find_page(PORT, tries=1)
    if not page:
        return ""
    cdp = _cdp.CDP(page["webSocketDebuggerUrl"], timeout=15)
    try:
        cdp.cmd("Network.enable")
        r = cdp.cmd("Network.getAllCookies")
        cookies = ((r.get("result") or {}).get("cookies")) or []
        return _session.flatten_cookies(cookies)
    finally:
        cdp.close()


def probe(cookie, uin, owner, csrf, region_id, tag):
    iso = lambda ts: datetime.datetime.fromtimestamp(
        ts, datetime.timezone(datetime.timedelta(hours=8))).strftime("%Y-%m-%dT%H:%M:%S+08:00")
    now = int(time.time() * 1000)
    url = ("https://console-hc.cloud.tencent.com/_api/monitor/GetMonitorData?timeout=30000&t=%d"
           "&uin=%s&ownerUin=%s&csrfCode=%s") % (now, uin, owner, csrf)
    end = int(time.time())
    start = end - 3600
    body = json.dumps({
        "cmd": "GetMonitorData", "serviceType": "monitor", "regionId": region_id,
        "data": {"Version": "2018-07-24", "Language": "zh-CN", "Namespace": "QCE/CMONGO",
                 "MetricName": "MonogdMaxCpuUsage",
                 "Instances": [{"Dimensions": [{"Name": "target", "Value": "cmgo-anbbj66h"}]}],
                 "Period": 60, "StartTime": iso(start), "EndTime": iso(end)},
    }).encode()
    req = urllib.request.Request(
        url, data=body,
        headers=_session.headers(cookie, "https://console.cloud.tencent.com/monitor/apm/system/list"))
    try:
        with urllib.request.urlopen(req, timeout=60) as r:
            resp = (json.loads(r.read().decode("utf-8", "replace")).get("Response") or {})
            dp = len(resp.get("DataPoints") or [])
            print("  [%s] cookie_len=%d DataPoints=%d err=%s" % (tag, len(cookie), dp, resp.get("Error")))
    except Exception as e:
        print("  [%s] ERR %s" % (tag, e))


def main():
    # 试探要打的是"页面刚抓到的 Cookie"，但 uin/ownerUin/csrfCode 用磁盘上的会话
    # （登录成功后后端会把它落盘；这里不自己重新解析，避免口径不一致）
    cfg, uin, owner = _session.session()
    csrf = cfg["csrfCode"]
    region_id = cfg.get("regionId", 4)

    cookie = ""
    for i in range(30):
        cookie = grab_cookie()
        if cookie and _session.ids_from_cookie(cookie)[0]:
            n = len({p.split("=")[0].strip() for p in cookie.split(";") if "=" in p})
            print("抓到会话（第 %d 次尝试），cookie 长度 %d，cookie 数 %d" % (i + 1, len(cookie), n))
            break
        time.sleep(1)
    if not cookie:
        print("没抓到（登录窗口没开，或页面不是腾讯域名）")
        return 1

    t0 = time.time()
    for off in (0, 10, 20, 30, 60):
        while time.time() - t0 < off:
            time.sleep(0.5)
        probe(cookie, uin, owner, csrf, region_id, "T+%ds" % off)
    return 0


if __name__ == "__main__":
    sys.exit(main())
