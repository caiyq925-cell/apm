"""极简 CDP 客户端：只用标准库，不依赖 websocket-client。

为什么不用 websocket-client：这是交接用的诊断工具箱，要求"clone 下来就能跑"。
多一个 pip 依赖就多一次"跑不起来"（实测就卡在这一步：`No module named 'websocket'`）。
这里只实现 CDP 用得到的部分：文本帧、分片、ping/pong、close；
客户端帧按 RFC6455 加掩码。

注意：Chromium 的 DevTools WebSocket 端点会拒绝带 `Origin` 头的握手，
所以这里**不发送** Origin —— 等价于 websocket-client 的 `suppress_origin=True`。

用法：
    from _cdp import find_page, CDP
    page = find_page(9223)                    # 找腾讯域名的 page target
    cdp = CDP(page["webSocketDebuggerUrl"])
    print(cdp.evaluate("document.title"))
    cdp.close()
"""
import base64
import json
import os
import socket
import struct
import time
import urllib.request
from urllib.parse import urlparse

DEFAULT_PORT = 9223  # 与 login.rs 的 CDP_PORT 一致

# 本地调试端口必须**绕开 HTTP 代理**。
# 这台机器上设了 HTTP_PROXY/HTTPS_PROXY 而没有 no_proxy，走默认 opener 时请求会被代理劫持：
# 实测 `http://127.0.0.1:9223/json/list` 返回 `502 Bad Gateway`，CDP 工具直接失效
# （现象很有误导性——端口明明在 LISTENING）。所以这里显式用一个空 ProxyHandler 的 opener。
# 注意 WebSocket 那条连接用的是裸 socket，本来就不受影响。
_NO_PROXY = urllib.request.build_opener(urllib.request.ProxyHandler({}))


class WSError(Exception):
    pass


class WS:
    """最小 WebSocket 客户端（仅文本帧）。"""

    def __init__(self, url, timeout=20):
        u = urlparse(url)
        host = u.hostname or "127.0.0.1"
        port = u.port or 80
        path = (u.path or "/") + (("?" + u.query) if u.query else "")

        self.sock = socket.create_connection((host, port), timeout=timeout)
        self.sock.settimeout(timeout)
        key = base64.b64encode(os.urandom(16)).decode()
        handshake = (
            "GET %s HTTP/1.1\r\n"
            "Host: %s:%d\r\n"
            "Upgrade: websocket\r\n"
            "Connection: Upgrade\r\n"
            "Sec-WebSocket-Key: %s\r\n"
            "Sec-WebSocket-Version: 13\r\n"
            "\r\n"
        ) % (path, host, port, key)
        self.sock.sendall(handshake.encode())

        buf = b""
        while b"\r\n\r\n" not in buf:
            chunk = self.sock.recv(4096)
            if not chunk:
                raise WSError("握手时连接被对端关闭")
            buf += chunk
        head, _, rest = buf.partition(b"\r\n\r\n")
        status_line = head.split(b"\r\n", 1)[0].decode("latin-1", "replace")
        if "101" not in status_line:
            raise WSError("WebSocket 握手失败: " + status_line)
        self._buf = rest

    # ---------- 底层 ----------

    def _read(self, n):
        while len(self._buf) < n:
            chunk = self.sock.recv(max(4096, n - len(self._buf)))
            if not chunk:
                raise WSError("连接已关闭")
            self._buf += chunk
        out, self._buf = self._buf[:n], self._buf[n:]
        return out

    def _send_frame(self, opcode, payload=b""):
        n = len(payload)
        if n < 126:
            hdr = struct.pack("!BB", 0x80 | opcode, 0x80 | n)
        elif n < 65536:
            hdr = struct.pack("!BBH", 0x80 | opcode, 0x80 | 126, n)
        else:
            hdr = struct.pack("!BBQ", 0x80 | opcode, 0x80 | 127, n)
        mask = os.urandom(4)
        masked = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
        self.sock.sendall(hdr + mask + masked)

    def send_text(self, text):
        self._send_frame(0x1, text.encode("utf-8"))

    def recv_text(self):
        """读一条完整文本消息（自动拼分片、自动回 pong）。"""
        data = b""
        while True:
            b0, b1 = self._read(2)
            fin, opcode = b0 & 0x80, b0 & 0x0F
            masked, n = b1 & 0x80, b1 & 0x7F
            if n == 126:
                n = struct.unpack("!H", self._read(2))[0]
            elif n == 127:
                n = struct.unpack("!Q", self._read(8))[0]
            mask = self._read(4) if masked else None
            payload = self._read(n) if n else b""
            if mask:
                payload = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))

            if opcode == 0x9:      # ping -> pong
                self._send_frame(0xA, payload)
                continue
            if opcode == 0x8:      # close
                raise WSError("对端关闭了连接")
            if opcode in (0x1, 0x0):   # text / continuation
                data += payload
                if fin:
                    return data.decode("utf-8", "replace")
                continue
            # 其它（二进制等）丢弃，继续等下一条

    def close(self):
        try:
            self._send_frame(0x8)
        except Exception:
            pass
        try:
            self.sock.close()
        except Exception:
            pass


# ---------- CDP 便利层 ----------


def targets(port=DEFAULT_PORT):
    try:
        url = "http://127.0.0.1:%d/json/list" % port
        with _NO_PROXY.open(url, timeout=3) as r:
            return json.load(r)
    except Exception:
        return []


def find_page(port=DEFAULT_PORT, contains="tencent.com", tries=50, delay=0.4):
    """轮询调试端口，找 URL 含 contains 的 page target。找不到返回 None。"""
    for _ in range(tries):
        for t in targets(port):
            if t.get("type") == "page" and contains in (t.get("url") or ""):
                return t
        time.sleep(delay)
    return None


class CDP:
    def __init__(self, ws_url, timeout=25):
        self.ws = WS(ws_url, timeout=timeout)
        self._id = 0

    def cmd(self, method, params=None, timeout=25):
        self._id += 1
        mid = self._id
        self.ws.send_text(json.dumps({"id": mid, "method": method, "params": params or {}}))
        deadline = time.time() + timeout
        while True:
            self.ws.sock.settimeout(max(1.0, deadline - time.time()))
            msg = json.loads(self.ws.recv_text())
            if msg.get("id") == mid:
                return msg

    def evaluate(self, expression, await_promise=True, timeout=90):
        r = self.cmd(
            "Runtime.evaluate",
            {"expression": expression, "awaitPromise": await_promise, "returnByValue": True},
            timeout=timeout,
        )
        result = (r.get("result") or {})
        if "value" in (result.get("result") or {}):
            return result["result"]["value"]
        if result.get("exceptionDetails"):
            raise WSError("JS 抛异常: " + json.dumps(
                result["exceptionDetails"], ensure_ascii=False)[:400])
        if r.get("error"):
            raise WSError("CDP 错误: " + json.dumps(r["error"], ensure_ascii=False)[:300])
        return None

    def wait_event(self, method, timeout=15):
        """阻塞读事件，直到出现指定 method 的消息（跳过响应与无关事件）。

        用途：监听 Network.requestWillBeSent 抓页面自己发的请求（旧网关的
        x-lid/x-life 头只有这里拿得到，performance 记录里没有头）。返回事件消息，
        超时返回 None。
        """
        deadline = time.time() + timeout
        while True:
            self.ws.sock.settimeout(max(1.0, deadline - time.time()))
            try:
                msg = json.loads(self.ws.recv_text())
            except WSError:
                return None
            if msg.get("method") == method:
                return msg

    def close(self):
        self.ws.close()
