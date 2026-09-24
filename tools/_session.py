"""会话读取（诊断脚本共用）。

改造后 `uin` / `ownerUin` **不再是 config.json 的字段**（见 docs/DESIGN.md 第二节），
它们每次从 Cookie 里解析。任何直接 `cfg["uin"]` 的脚本都会 KeyError —— 统一走这里。

解析口径与 Rust `Config::extract_ids_from_cookie` 逐字对齐；Cookie 扁平化与
`login.rs::build_cookie` 对齐（跳过空值、同名优先取 cloud.tencent.com 那份）。
"""
import json
import os

_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEFAULT_CFG = os.path.join(_ROOT, "dist", "config.json")

# 与 Rust 侧保持一致的伪装头（Referer 可按链路覆盖）
_UA = ("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
       "(KHTML, like Gecko) Chrome/153.0.0.0 Safari/537.36")


def load_config(path=None):
    with open(path or DEFAULT_CFG, encoding="utf-8") as f:
        return json.load(f)


def ids_from_cookie(cookie):
    """返回 (uin, owner_uin)。键名大小写敏感（与 Rust 一致）。

    uin 去前导 o/O；ownerUin 去前导 O/o 与后缀 G/g。
    """
    uin = owner = ""
    for pair in (cookie or "").split(";"):
        if "=" not in pair:
            continue
        k, v = pair.strip().split("=", 1)
        if k == "uin":
            uin = v.strip().lstrip("oO")
        elif k == "ownerUin":
            s = v.strip()
            if s[:1] in ("O", "o"):
                s = s[1:]
            if s[-1:] in ("G", "g"):
                s = s[:-1]
            owner = s
    return uin, owner


def flatten_cookies(cookies):
    """把 CDP `Network.getAllCookies` 的结果扁平化成 Cookie 头。

    与 `build_cookie` 同一套规则：只看 tencent.com 域、跳过空值、
    同名优先取控制台域那份 —— 否则 `.tencent.com` 上的 `uin=`（空）会把
    `.cloud.tencent.com` 上的真值顶掉，抓出来的会话服务端一律不认。
    """
    order, best = [], {}
    for c in cookies or []:
        domain = c.get("domain") or ""
        if not domain.endswith("tencent.com"):
            continue
        name, value = c.get("name") or "", c.get("value") or ""
        if not name or not value:
            continue
        score = 2 if "cloud.tencent.com" in domain else 1
        if best.get(name, (0, ""))[0] >= score:
            continue
        if name not in best:
            order.append(name)
        best[name] = (score, value)
    return "; ".join("%s=%s" % (n, best[n][1]) for n in order)


def session(path=None):
    """返回 (cfg, uin, owner_uin)。会话不完整时直接退出并说明缺什么。"""
    cfg = load_config(path)
    uin, owner = ids_from_cookie(cfg.get("cookie") or "")
    missing = [n for n, v in (
        ("cookie", cfg.get("cookie")),
        ("csrfCode", cfg.get("csrfCode")),
        ("uin（从 cookie 解析）", uin),
        ("ownerUin（从 cookie 解析）", owner),
    ) if not v]
    if missing:
        raise SystemExit("会话不完整，缺：" + "、".join(missing) + " —— 先在界面点「扫码登录」")
    return cfg, uin, owner


def headers(cookie, referer="https://console.cloud.tencent.com/monitor/apm/system/list"):
    return {
        "Content-Type": "application/json",
        "Origin": "https://console.cloud.tencent.com",
        "Referer": referer,
        "X-Requested-With": "XMLHttpRequest",
        "User-Agent": _UA,
        "Cookie": cookie,
    }


def mask(tok, keep=6):
    """令牌打码：只留长度和头几个字符。令牌与会话绑定，等同凭证，不要整串打印。"""
    if not tok:
        return "(空)"
    return "%s…(len=%d)" % (tok[:keep], len(tok))
