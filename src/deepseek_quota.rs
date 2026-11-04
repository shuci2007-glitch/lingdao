//! DeepSeek 账户余额。
//!
//! 走 DeepSeek 官方接口 `GET /user/balance`，只需要用户自己的 API Key，
//! 不读浏览器的任何会话 Cookie，也不碰其他客户端的登录文件。
//! Key 存在 Windows 凭据管理器，和邮箱授权码同一套方式。

use super::{Account, Snapshot, Window};
use serde_json::Value;
use std::io::Read;

const SERVICE: &str = "Daozai.DeepSeek";
const ENDPOINT: &str = "https://api.deepseek.com/user/balance";
const KEY_HINT: &str = "请在 DeepSeek 开放平台创建 API Key 后填到这里";

pub fn save_key(id: &str, key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.len() < 8 || key.len() > 256 || key.chars().any(char::is_whitespace) {
        return Err("API Key 格式不正确".into());
    }
    keyring::Entry::new(SERVICE, id)
        .and_then(|entry| entry.set_password(key))
        .map_err(|e| format!("保存 API Key 失败：{e}"))
}

pub fn forget(account: &Account) {
    if let Ok(entry) = keyring::Entry::new(SERVICE, &account.id) {
        let _ = entry.delete_credential();
    }
}

fn key(id: &str) -> Result<String, String> {
    keyring::Entry::new(SERVICE, id)
        .and_then(|entry| entry.get_password())
        .map_err(|_| KEY_HINT.to_string())
}

/// 账号 id 用 key 的 SHA-256 前 12 位，同一个 Key 重复添加只会得到同一个账号。
pub fn slot(key: &str) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, key.trim().as_bytes());
    digest.as_ref()[..6].iter().map(|b| format!("{b:02x}")).collect()
}

fn money(value: &Value) -> Option<f64> {
    value.as_str().and_then(|s| s.trim().parse().ok())
}

pub fn parse(value: &Value) -> Result<Snapshot, String> {
    let infos = value["balance_infos"]
        .as_array()
        .ok_or("服务没有返回余额信息")?;
    let info = infos
        .iter()
        .find(|item| item["currency"].as_str() == Some("CNY"))
        .or_else(|| infos.first())
        .ok_or("服务没有返回余额信息")?;
    let total = money(&info["total_balance"]).ok_or("余额字段格式异常")?;
    let granted = money(&info["granted_balance"]).unwrap_or(0.0);
    let topped = money(&info["topped_up_balance"]).unwrap_or(0.0);
    let symbol = match info["currency"].as_str() {
        Some("USD") => "$",
        _ => "¥",
    };
    let available = value["is_available"].as_bool().unwrap_or(total > 0.0);
    let detail = |label: &str, amount: f64| Window {
        label: label.to_owned(),
        seconds: None,
        // 明细行只作展示，圆环用不到百分比。
        remaining: amount as f32,
        reset: None,
    };
    Ok(Snapshot {
        plan: format!("{symbol}{total:.2}"),
        windows: vec![Window {
            label: "账户余额".into(),
            seconds: None,
            remaining: if available { 100.0 } else { 0.0 },
            reset: None,
        }],
        extra: vec![detail("充值", topped), detail("赠送", granted)],
        at: chrono::Utc::now().timestamp(),
    })
}

pub fn fetch(agent: &ureq::Agent, account: &Account) -> Result<Snapshot, (String, u64)> {
    let key = key(&account.id).map_err(|error| (error, 0))?;
    let response = agent
        .get(ENDPOINT)
        .set("Authorization", &format!("Bearer {key}"))
        .set("Accept", "application/json")
        .set("User-Agent", "Daozai")
        .call()
        .map_err(|error| match error {
            ureq::Error::Status(401 | 403, _) => ("API Key 无效或已被撤销，请重新填写".to_string(), 0),
            ureq::Error::Status(402, _) => ("账户余额不足".to_string(), 300),
            ureq::Error::Status(429, response) => (
                "请求过于频繁，稍后自动重试".to_string(),
                response
                    .header("Retry-After")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(180)
                    .clamp(30, 3600),
            ),
            ureq::Error::Status(code, _) => (format!("DeepSeek 服务暂不可用（{code}）"), 60),
            _ => ("无法连接 DeepSeek，请检查网络或代理".to_string(), 30),
        })?;
    let mut body = String::new();
    response
        .into_reader()
        .take(1024 * 1024)
        .read_to_string(&mut body)
        .map_err(|_| ("读取余额失败".to_string(), 30))?;
    let value: Value =
        serde_json::from_str(&body).map_err(|_| ("服务没有返回有效的余额数据".to_string(), 60))?;
    parse(&value).map_err(|error| (error, 60))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_cny_balance_and_extra_lines() {
        let value = json!({
            "is_available": true,
            "balance_infos": [{
                "currency": "CNY", "total_balance": "110.00",
                "granted_balance": "10.00", "topped_up_balance": "100.00"
            }]
        });
        let snapshot = parse(&value).unwrap();
        assert_eq!(snapshot.plan, "¥110.00");
        assert_eq!(snapshot.windows.len(), 1);
        assert_eq!(snapshot.windows[0].remaining, 100.0);
        assert_eq!(snapshot.extra.len(), 2);
        assert_eq!(snapshot.extra[1].remaining, 10.0);
    }

    #[test]
    fn unavailable_balance_drops_the_ring() {
        let value = json!({
            "is_available": false,
            "balance_infos": [{"currency": "USD", "total_balance": "0.00", "granted_balance": "0", "topped_up_balance": "0"}]
        });
        let snapshot = parse(&value).unwrap();
        assert_eq!(snapshot.plan, "$0.00");
        assert_eq!(snapshot.windows[0].remaining, 0.0);
    }

    #[test]
    fn rejects_payload_without_balance() {
        assert!(parse(&json!({"is_available": true})).is_err());
        assert!(parse(&json!({"balance_infos": []})).is_err());
    }

    #[test]
    fn same_key_maps_to_the_same_slot() {
        assert_eq!(slot("sk-abcdefgh"), slot("  sk-abcdefgh  "));
        assert_ne!(slot("sk-abcdefgh"), slot("sk-abcdefgi"));
        assert_eq!(slot("sk-abcdefgh").len(), 12);
    }
}
