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
    pub region_id: i64,      // Cookie 模式控制台接口的数字地域 ID，默认 4（上海）
    pub instance_id: String, // 业务系统 ID（team），如 apm-tHxaVZjHH
    pub auth_mode: String,   // "secret" | "cookie"
    pub secret_id: String,
    pub secret_key: String,
    pub cookie: String,
    pub uin: String,
    pub owner_uin: String,
    pub csrf_code: String,
    // 启用的数据源（可多选）："apm" | "mysql" | "redis" | "mongodb"
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

impl Config {
    pub fn with_defaults() -> Self {
        Config {
            region: "ap-shanghai".into(),
            region_id: 4,
            auth_mode: "secret".into(),
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

    /// 从 Cookie 串解析 uin / ownerUin（去掉控制台加的前后缀），字段为空时生效
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
        let uin = if self.uin.is_empty() { uin } else { self.uin.clone() };
        let owner = if self.owner_uin.is_empty() { owner } else { self.owner_uin.clone() };
        (uin, owner)
    }
}
