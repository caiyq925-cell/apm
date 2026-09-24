// 便携式配置：存于 exe 同目录 config.json
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct MetricDef {
    pub name: String,
    pub view: String,
    pub cn: String,
}

impl Default for MetricDef {
    fn default() -> Self {
        MetricDef { name: String::new(), view: "service_metric".into(), cn: String::new() }
    }
}

/// 场景预设：启用的数据源 + APM 应用 + 数据库实例 + 指标的快照
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Scenario {
    pub name: String,
    /// 该场景启用的数据源（如 ["apm","mysql"]）
    pub sources: Vec<String>,
    pub apps: Vec<String>,
    pub db_instances: BTreeMap<String, Vec<String>>,
    pub metrics: Vec<MetricDef>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    pub region: String,      // 地域，默认 ap-shanghai
    pub region_id: i64,      // 控制台接口的数字地域 ID，默认 4（上海）
    pub instance_id: String, // 业务系统 ID（team），如 apm-tHxaVZjHH
    // ---- 会话（只由后端写入：扫码登录 / 后台预刷新；uin/ownerUin 每次从 cookie 解析，不落字段）----
    pub cookie: String,
    pub csrf_code: String,
    /// 会话获取时间（Unix 秒），0 表示未知
    pub session_fetched_at: i64,
    // ---- 会话维护（界面上可调）----
    /// 会话年龄超过这个分钟数就值得预刷新
    #[serde(default = "default_refresh_minutes")]
    pub session_refresh_minutes: i64,
    /// 预刷新失败后的首次退避间隔（分钟）
    #[serde(default = "default_backoff_minutes")]
    pub session_refresh_backoff_minutes: i64,
    /// 连续失败达到这个次数就停止后台自动刷新
    #[serde(default = "default_max_fails")]
    pub session_refresh_max_fails: i64,
    /// 后台自动预刷新总开关
    #[serde(default = "default_true")]
    pub session_auto_refresh: bool,
    /// 扫码登录窗口里自动点击「微信快捷登录」（已绑定微信时免扫码），仅手动可见窗口生效
    #[serde(default = "default_true")]
    pub session_auto_quick_login: bool,
    // 启用的数据源（可多选）："mysql" | "redis" | "mongodb" | "container"
    #[serde(default = "default_sources")]
    pub enabled_sources: Vec<String>,
    pub quick_range: String,  // 15m/30m/1h/2h/6h/12h/24h/today/yesterday/3d/7d
    pub use_custom: bool,
    pub custom_start: String, // YYYY-MM-DDTHH:mm
    pub custom_end: String,
    pub selected_apps: Vec<String>,
    pub selected_metrics: Vec<MetricDef>,
    pub metric_cache: Vec<MetricDef>,
    // 数据库类型 -> 已选实例 ID 列表
    pub selected_db_instances: BTreeMap<String, Vec<String>>,
    // 场景预设与当前激活场景（空/"自定义" 表示手动模式）
    pub scenarios: Vec<Scenario>,
    pub active_scenario: String,
    // 容器服务（TKE）：当前集群/命名空间与已选工作负载
    pub container_cluster: String,
    pub container_namespace: String,
    pub selected_deployments: Vec<String>,
}

fn default_sources() -> Vec<String> {
    vec!["container".to_string()]
}

fn default_refresh_minutes() -> i64 {
    10
}
fn default_backoff_minutes() -> i64 {
    2
}
fn default_max_fails() -> i64 {
    3
}
fn default_true() -> bool {
    true
}

impl Config {
    pub fn with_defaults() -> Self {
        Config {
            region: "ap-shanghai".into(),
            region_id: 4,
            session_refresh_minutes: default_refresh_minutes(),
            session_refresh_backoff_minutes: default_backoff_minutes(),
            session_refresh_max_fails: default_max_fails(),
            session_auto_refresh: true,
            session_auto_quick_login: true,
            enabled_sources: default_sources(),
            quick_range: "1h".into(),
            ..Default::default()
        }
    }

    fn path() -> PathBuf {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."))
            .join("config.json")
    }

    pub fn load() -> Self {
        let p = Self::path();
        match std::fs::read_to_string(&p) {
            Ok(s) => {
                let mut c: Config = serde_json::from_str(&s).unwrap_or_else(|_| Config::with_defaults());
                if c.region_id == 0 {
                    c.region_id = 4;
                }
                c
            }
            Err(_) => Config::with_defaults(),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(Self::path(), json).map_err(|e| e.to_string())
    }

    /// 用扫码登录取回的会话更新配置（只存 cookie + csrfCode + 获取时间）。
    /// 空值不覆盖已有值：抓取偶发拿到半初始化的 Cookie，直接覆盖会把能用的会话清掉。
    /// 返回被保留下来的字段名，便于打日志。
    pub fn apply_login(&mut self, cookie: String, csrf_code: String) -> Vec<&'static str> {
        let mut kept = Vec::new();
        if !cookie.trim().is_empty() {
            self.cookie = cookie;
        } else if !self.cookie.trim().is_empty() {
            kept.push("cookie");
        }
        if !csrf_code.trim().is_empty() {
            self.csrf_code = csrf_code;
        } else if !self.csrf_code.trim().is_empty() {
            kept.push("csrfCode");
        }
        self.session_fetched_at = now_secs();
        kept
    }

    /// 保存前调用：用磁盘上的配置做基准，只接受前端传来的"用户配置"字段。
    /// 会话字段（cookie/csrfCode/获取时间）永远以后端为准，前端无法写坏。
    pub fn merge_user_config(&self, incoming: Config) -> Config {
        Config {
            cookie: self.cookie.clone(),
            csrf_code: self.csrf_code.clone(),
            session_fetched_at: self.session_fetched_at,
            ..incoming
        }
    }

    /// 会话是否可用（cookie 里能解析出 uin/ownerUin，且有 csrfCode）
    pub fn session_ready(&self) -> bool {
        let (uin, owner) = self.extract_ids_from_cookie();
        !self.cookie.trim().is_empty()
            && !uin.is_empty()
            && !owner.is_empty()
            && !self.csrf_code.trim().is_empty()
    }

    /// 会话年龄（秒）；未知时返回 i64::MAX，等价于"早就该刷新了"
    pub fn session_age_secs(&self) -> i64 {
        if self.session_fetched_at <= 0 {
            i64::MAX
        } else {
            (now_secs() - self.session_fetched_at).max(0)
        }
    }

    /// 从 Cookie 串解析 uin / ownerUin（去掉控制台加的前后缀）
    pub fn extract_ids_from_cookie(&self) -> (String, String) {
        let mut uin = String::new();
        let mut owner = String::new();
        for pair in self.cookie.split(';') {
            let mut it = pair.trim().splitn(2, '=');
            let (k, v) = match (it.next(), it.next()) {
                (Some(k), Some(v)) => (k, v),
                _ => continue,
            };
            match k {
                "uin" => uin = v.trim().trim_start_matches(['o', 'O']).to_string(),
                "ownerUin" => {
                    let s = v.trim();
                    let s = s.strip_prefix(['O', 'o']).unwrap_or(s);
                    let s = s.strip_suffix(['G', 'g']).unwrap_or(s);
                    owner = s.to_string();
                }
                _ => {}
            }
        }
        (uin, owner)
    }

    /// 旧网关 `/cgi/capi` 的 csrfCode = `bkn(skey)`（抓包实锤，见 `login::bkn`）。
    ///
    /// 会话有效期内它是确定的，会话轮换后跟着变——所以**每次发请求现算**，
    /// 不依赖登录时存下的 `csrf_code`（那个可能已经随 skey 轮换过期）。取不到 skey
    /// 时退回存量字段，两边都没有才判"缺 csrf"。
    pub fn capi_csrf(&self) -> String {
        let skey = self
            .cookie
            .split(';')
            .map(str::trim)
            .find_map(|p| p.strip_prefix("skey="))
            .filter(|s| !s.is_empty());
        match skey {
            Some(s) if crate::login::bkn(s) > 0 => crate::login::bkn(s).to_string(),
            _ => self.csrf_code.clone(),
        }
    }
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// uin/ownerUin 不再是字段，每次从 Cookie 派生，且要剥掉控制台加的前后缀
    #[test]
    fn extracts_ids_from_cookie_stripping_console_affixes() {
        let c = Config {
            cookie: "other=1; uin=o765378326; ownerUin=O100012781415G".into(),
            ..Config::with_defaults()
        };
        let (uin, owner) = c.extract_ids_from_cookie();
        assert_eq!(uin, "765378326");
        assert_eq!(owner, "100012781415");
    }

    #[test]
    fn extract_ids_is_empty_for_incomplete_cookie() {
        let c = Config { cookie: "other=1".into(), ..Config::with_defaults() };
        assert_eq!(c.extract_ids_from_cookie(), (String::new(), String::new()));
    }

    #[test]
    fn session_ready_requires_cookie_ids_and_csrf() {
        let mut c = Config::with_defaults();
        assert!(!c.session_ready(), "空配置不该 ready");

        c.cookie = "uin=o1; ownerUin=O2G".into();
        assert!(!c.session_ready(), "缺 csrfCode 不该 ready");

        c.csrf_code = "t".into();
        assert!(c.session_ready());
    }

    /// capi_csrf = bkn(skey)（抓包实锤的算法），不是登录时存下的那种固定值
    #[test]
    fn capi_csrf_is_bkn_of_skey() {
        let mut c = Config::with_defaults();
        c.cookie = "skey=JDf9UdfZDWrSYaYlU5lYZGoE3PwBB96Es4spg6UJf2U_; uin=o1".into();
        // 该 skey 的 bkn 正是抓包里页面所有请求用的 csrfCode
        assert_eq!(c.capi_csrf(), "1732802354");
        // 无 skey 时退回登录时存下的字段
        c.cookie = "uin=o1".into();
        c.csrf_code = "12345".into();
        assert_eq!(c.capi_csrf(), "12345");
        // 都没有要能自证"缺 csrf"，而不是 panic
        c.csrf_code.clear();
        assert!(c.capi_csrf().is_empty());
    }

    #[test]
    fn unknown_session_age_is_max() {
        let mut c = Config::with_defaults();
        c.session_fetched_at = 0;
        assert_eq!(c.session_age_secs(), i64::MAX);
    }

    /// 核心不变量：保存时会话字段一律用磁盘上的，前端传来的空值不能把它写坏
    /// （改造前真的发生过：前端用空值覆盖掉可用会话，之后所有接口都报"无法确定 uin/ownerUin"）
    #[test]
    fn merge_user_config_keeps_disk_session_but_takes_user_fields() {
        let disk = Config {
            cookie: "disk-cookie".into(),
            csrf_code: "disk-csrf".into(),
            session_fetched_at: 12345,
            region: "ap-shanghai".into(),
            ..Config::with_defaults()
        };
        let incoming = Config {
            cookie: String::new(),
            csrf_code: String::new(),
            session_fetched_at: 0,
            region: "ap-beijing".into(),
            ..Config::with_defaults()
        };
        let merged = disk.merge_user_config(incoming);
        assert_eq!(merged.cookie, "disk-cookie");
        assert_eq!(merged.csrf_code, "disk-csrf");
        assert_eq!(merged.session_fetched_at, 12345);
        assert_eq!(merged.region, "ap-beijing", "用户配置要用前端最新的");
    }

    #[test]
    fn apply_login_does_not_overwrite_with_empty_values() {
        let mut c = Config {
            cookie: "old-cookie".into(),
            csrf_code: "old-csrf".into(),
            ..Config::with_defaults()
        };
        let kept = c.apply_login(String::new(), String::new());
        assert_eq!(c.cookie, "old-cookie");
        assert_eq!(c.csrf_code, "old-csrf");
        assert_eq!(kept, vec!["cookie", "csrfCode"]);
        assert!(c.session_fetched_at > 0, "时间戳仍应更新");
    }

    #[test]
    fn apply_login_takes_non_empty_values() {
        let mut c = Config::with_defaults();
        let kept = c.apply_login("new-cookie".into(), "new-csrf".into());
        assert_eq!(c.cookie, "new-cookie");
        assert_eq!(c.csrf_code, "new-csrf");
        assert!(kept.is_empty());
    }
}
