# -*- coding: utf-8 -*-
# Directly probe the WeChat local service (no browser) to see if it responds
# to the qrconnect check-login request at all.
import json
import ssl
import urllib.request

ctx = ssl.create_default_context()
ctx.check_hostname = False
ctx.verify_mode = ssl.CERT_NONE

body = json.dumps({"apiname": "qrconnectchecklogin",
                   "jsdata": {"appid": "wxca396cd1df083b9d"}}).encode()

for port in [14013, 14014, 14015, 13013, 13014, 13015, 14016, 14019, 14022, 14023]:
    url = "https://localhost.weixin.qq.com:%d/api/check-login" % port
    try:
        req = urllib.request.Request(url, data=body, method="POST",
                                     headers={"Content-Type": "application/json",
                                              "User-Agent": "Mozilla/5.0"})
        with urllib.request.urlopen(req, timeout=4, context=ctx) as r:
            print(port, "-> HTTP", r.status, repr(r.read(200)))
    except Exception as e:
        print(port, "-> ERR", type(e).__name__, str(e)[:90])
