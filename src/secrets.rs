//! Windows 凭据管理器访问。
//!
//! v0.6.52 起服务名从 `Daozai.*` 改成 `Lingdao.*`。为了不让人重输授权码，
//! 读取时先找新名，找不到再回退旧名，并把读到的旧凭据顺手写到新名下。
#![cfg(windows)]

/// `Lingdao.Mail` → `Daozai.Mail`（旧版本用过的服务名）。
fn legacy(service: &str) -> String {
    service.replacen("Lingdao.", "Daozai.", 1)
}

/// 读一条凭据；命中旧名时自动迁移到新名。
pub fn get(service: &str, user: &str) -> Result<String, keyring::Error> {
    let entry = keyring::Entry::new(service, user)?;
    match entry.get_password() {
        Ok(secret) => Ok(secret),
        Err(keyring::Error::NoEntry) => {
            let old = keyring::Entry::new(&legacy(service), user)?;
            match old.get_password() {
                Ok(secret) => {
                    let _ = entry.set_password(&secret);
                    Ok(secret)
                }
                Err(error) => Err(error),
            }
        }
        Err(error) => Err(error),
    }
}

/// 写入新名下的凭据。
pub fn set(service: &str, user: &str, secret: &str) -> Result<(), keyring::Error> {
    keyring::Entry::new(service, user)?.set_password(secret)
}

/// 删除凭据，新旧名一起删，避免凭据管理器里留孤儿条目。
pub fn delete(service: &str, user: &str) {
    for name in [service.to_owned(), legacy(service)] {
        if let Ok(entry) = keyring::Entry::new(&name, user) {
            let _ = entry.delete_credential();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::legacy;

    #[test]
    fn legacy_name_only_rewrites_the_service_prefix() {
        assert_eq!(legacy("Lingdao.Mail"), "Daozai.Mail");
        assert_eq!(legacy("Lingdao.GrokBot"), "Daozai.GrokBot");
        // 不认识的格式保持原样，避免误伤。
        assert_eq!(legacy("Other.Service"), "Other.Service");
    }
}
