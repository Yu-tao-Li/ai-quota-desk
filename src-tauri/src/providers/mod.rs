//! 三家套餐额度查询的公共类型与工具。

pub mod codex;
pub mod deepseek;
pub mod glm;
pub mod kimi;
pub mod luchikey;

use serde::Serialize;

/// 单个额度窗口（如 5 小时窗口、每周额度）。
#[derive(Serialize, Clone, Debug)]
pub struct QuotaWindow {
    /// 窗口名，由重置时间推断：5小时窗口 / 每周额度 / 每月额度
    pub label: String,
    /// 已用百分比 0-100
    pub used_percent: f64,
    /// 剩余百分比 0-100
    pub remaining_percent: f64,
    /// 用量文本，如 "7,903 / 12,000"
    pub used_text: Option<String>,
    /// 重置时间（Unix 毫秒）
    pub resets_at_ms: Option<i64>,
}

/// 一张额度重置卡（ChatGPT Codex 的 reset credit）。
#[derive(Serialize, Clone, Debug)]
pub struct ResetCredit {
    /// 第几张（1 起）
    pub index: i64,
    /// 到期时间（Unix 毫秒）；None = 永不过期
    pub expires_at_ms: Option<i64>,
}

/// 一个套餐的查询结果。
#[derive(Serialize, Clone, Debug)]
pub struct ProviderQuota {
    pub id: String,
    pub name: String,
    pub ok: bool,
    pub error: Option<String>,
    /// 套餐等级：GLM "pro" / ChatGPT "plus" 等
    pub plan: Option<String>,
    /// 附加信息（credits 余额、加油包、并发数等）
    pub extra: Option<String>,
    pub windows: Vec<QuotaWindow>,
    /// 重置卡列表（目前只有 ChatGPT Codex 有）
    pub credits: Vec<ResetCredit>,
    pub fetched_at_ms: i64,
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 按重置时间推断窗口名。厂商返回的 unit 枚举不稳定
/// （智谱新套餐里周/月的枚举和旧文档对不上），倒计时推断更可靠。
pub fn label_for_reset(resets_at_ms: Option<i64>, now_ms: i64) -> String {
    match resets_at_ms {
        Some(t) => {
            let d = t - now_ms;
            if d <= 0 {
                "即将重置".to_string()
            } else if d < 6 * 3600 * 1000 {
                "5小时窗口".to_string()
            } else if d < 8 * 24 * 3600 * 1000 {
                "每周额度".to_string()
            } else if d < 32 * 24 * 3600 * 1000 {
                "每月额度".to_string()
            } else {
                "当前周期".to_string()
            }
        }
        None => "当前周期".to_string(),
    }
}

/// 千分位格式化数字。
pub fn fmt_num(n: f64) -> String {
    let s = if n.fract() == 0.0 {
        format!("{}", n as i64)
    } else {
        format!("{:.2}", n)
    };
    // 简单千分位：整数部分插逗号
    let (int, frac) = match s.split_once('.') {
        Some((i, f)) => (i.to_string(), Some(f.to_string())),
        None => (s, None),
    };
    let negative = int.starts_with('-');
    let digits = int.trim_start_matches('-');
    let mut grouped = String::new();
    for (idx, ch) in digits.chars().enumerate() {
        if idx > 0 && (digits.len() - idx) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    format!("{}{}{}", if negative { "-" } else { "" }, grouped, frac.map(|f| format!(".{f}")).unwrap_or_default())
}

/// 错误信息对用户友好化：截断过长的上游报错。
pub fn short_err(prefix: &str, e: impl std::fmt::Display) -> String {
    let msg = e.to_string();
    let short: String = msg.chars().take(160).collect();
    format!("{prefix}: {short}")
}
