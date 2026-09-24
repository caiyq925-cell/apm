//! 凭证失效的统一判定。
//!
//! 低层遇到"会话不可用、需要重新登录"的错误时用 [`cred`] 打标记；上层用
//! [`is_session_invalid`] 决定是否拉起扫码登录。不用纯文案匹配的原因：
//! 控制台 /_api 那条链路的响应里 `Code` 可能是 `"Unknown"`，只剩中文 Message 可判，
//! 所以把"结构化 code 优先、已知文案兜底"集中在这一处，而不是散在各处 `.contains("1216")`。
//!
//! 这里有一条容易踩的界线：**旧网关 `/cgi/capi` 的令牌失效不算会话失效**。
//! 它的令牌寿命只有十来分钟，换新会话也照样可能被拒；把它判成会话失效会让容器利用率
//! 一失败就中断整次统计。这类错误走 [`is_capi_token_stale`]，只用来决定"换个令牌重试一次"。

/// 标记（用不可见控制字符，不会和正常文案撞）
pub const PREFIX: &str = "\u{1}CRED\u{1}";

/// 给"需要重新登录"的错误打标记
pub fn cred(msg: impl std::fmt::Display) -> String {
    format!("{}{}", PREFIX, msg)
}

pub fn is_cred(e: &str) -> bool {
    e.contains(PREFIX)
}

/// 去掉标记，用于界面与日志展示
pub fn strip(e: &str) -> String {
    e.replace(PREFIX, "")
}

// ---- 下面四个判定目前没有生产调用方（前端的重登由错误串里的 CRED 标记驱动，
//      容器链路 1216 根因已修、不再需要"换令牌重试"的判定），**保留它们与测试**：
//      它们编码了"什么算会话失效 / 什么绝不算"的语义，将来恢复相关流程时直接可用。----

/// 传输/网络类错误——这类绝不能触发扫码登录
#[allow(dead_code)]
pub fn is_transport(e: &str) -> bool {
    const MARKERS: [&str; 5] = [
        "请求失败",
        "响应非 JSON",
        "连接调试端口",
        "调试通道",
        "连接不上调试端口",
    ];
    MARKERS.iter().any(|m| e.contains(m))
}

/// 服务端返回里的会话失效标志
///
/// 这里**故意不含 `code=1216`**：那是旧网关 `/cgi/capi` 的码（且 2026-09-24 查明其真根因
/// 是请求体包装、不是会话），换一份全新会话、甚至用控制台页面自己那份令牌，都可能照样出现，
/// 不代表整机会话坏了（见 [`is_capi_token_stale`]）。把两者混为一谈的代价是：容器利用率一失败
/// 就把整次统计判成"需要重新登录"并中断，连已经拿到的数据库结果一起丢掉。
#[allow(dead_code)]
pub fn server_says_invalid(e: &str) -> bool {
    const MARKERS: [&str; 5] = [
        "code=9", // 控制台 /_api：CSRF 或登录态校验失败
        "登录态过期",
        "登录态验证失败",
        "验证CSRF失败",
        "请重新登录",
    ];
    MARKERS.iter().any(|m| e.contains(m))
}

/// 旧网关（`/cgi/capi`）**回了非 0 的 code** —— 即"它拒了我们这次请求"。
///
/// 容器链路据此决定要不要走"页面自己发"的退路（`container.rs::pod_util_metrics`）。
#[allow(dead_code)]
pub fn is_capi_token_stale(e: &str) -> bool {
    e.contains("旧网关接口错误")
}

/// 统一判定：是否属于"会话失效，需要重新登录"
#[allow(dead_code)]
pub fn is_session_invalid(e: &str) -> bool {
    if is_transport(e) {
        return false;
    }
    is_cred(e) || server_says_invalid(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 这条是整个改造的核心不变量：旧网关令牌没被接受**不算**会话失效。
    /// 破了它，容器利用率一失败就会中断整次统计，把数据库/APM 已经拿到的结果一起丢掉。
    #[test]
    fn capi_1216_is_not_session_invalid() {
        let e = "旧网关接口错误 code=1216: 不合法的云 API 类型（容器 CPU/内存利用率依赖该网关的短寿命令牌）";
        assert!(is_capi_token_stale(e), "应判为令牌需重试");
        assert!(!is_session_invalid(e), "绝不该判成会话失效");
        assert!(!server_says_invalid(e));
        assert!(!is_cred(e));
    }

    #[test]
    fn capi_preheat_failure_is_neither_cred_nor_token_stale() {
        // 预热失败 = "页面没发出请求"（多半是登录态/页面打不开）：换令牌重试没意义，
        // 也不能当成会话失效（否则会误弹登录窗口）。每个工作负载重试一次还会白等几十秒。
        let e = "没能从控制台页面取到旧网关令牌（页面没有发出 dashboard 请求：可能是工作负载页打不开，\
                 也可能登录窗口里没有登录态 —— 展开「设置」点一次「扫码登录」即可）";
        assert!(!is_capi_token_stale(e), "不该触发换令牌重试");
        assert!(!is_session_invalid(e), "也不该判成会话失效");
    }

    /// 但"页面被送到登录页"是**真的**需要重新登录，必须标 cred（前端据此拉起扫码窗口）
    #[test]
    fn capi_login_page_is_cred() {
        let e = cred(
            "登录窗口里没有登录态（控制台页面被送到了 https://cloud.tencent.com/login?s=1）：\
             请在「设置」里点「扫码登录」完成扫码",
        );
        assert!(is_session_invalid(&e));
        assert!(!is_capi_token_stale(&e));
    }

    #[test]
    fn api_session_failure_is_session_invalid() {
        // /_api 的真实形态：api_err 会先打标记
        assert!(is_session_invalid(&cred("接口错误 9: 登录态验证失败")));
        // Code 是 Unknown 时只剩中文可判
        assert!(server_says_invalid("接口错误 Unknown: 登录态验证失败"));
        assert!(server_says_invalid("接口错误 Unknown: 验证CSRF失败"));
        assert!(server_says_invalid("接口错误 Unknown: 请重新登录"));
        // 这些不该被误认成令牌问题
        assert!(!is_capi_token_stale(&cred("接口错误 9: 登录态验证失败")));
    }

    /// 网络抖动绝不能弹登录窗口
    #[test]
    fn transport_never_triggers_login() {
        for e in [
            "请求失败: connection reset by peer",
            "响应非 JSON (HTTP 502): <html>",
            "连接不上调试端口 127.0.0.1:9223（请确认 tauri.config 的 additionalBrowserArgs）",
            "调试通道错误: broken pipe",
        ] {
            assert!(!is_session_invalid(e), "{} 不该判成会话失效", e);
        }
    }

    /// 参数/权限类错误也不能拿去触发登录
    #[test]
    fn parameter_and_permission_errors_are_not_cred() {
        for e in [
            "InvalidParameterValue: 参数取值错误",
            "UnauthorizedOperation.CamNoAuth: 没有权限",
            "未选择对象",
        ] {
            assert!(!is_session_invalid(e), "{} 不该判成会话失效", e);
        }
    }

    #[test]
    fn strip_removes_invisible_marker() {
        let e = cred("缺少 csrfCode：请重新登录");
        assert!(is_cred(&e));
        assert_eq!(strip(&e), "缺少 csrfCode：请重新登录");
        assert!(!strip(&e).contains('\u{1}'));
    }
}
