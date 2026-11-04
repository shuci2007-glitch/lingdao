//! 验证码显示隐私：只做「显示满 N 秒后遮罩」，不推断验证码是否已失效。
use std::collections::{HashMap, HashSet};
use crate::mail::Item;

#[derive(Default)]
pub(crate) struct Privacy {
    /// 首次看到某验证码的时间。
    seen: HashMap<(String, u32), f64>,
    /// 用户手动点开后的临时可见截止时间。
    revealed: HashMap<(String, u32), f64>,
}

fn key(item: &Item) -> (String, u32) {
    (item.account.to_ascii_lowercase(), item.uid)
}

/// 把文本里的验证码替换成圆点，兼容全角 / 零宽 / 空格分隔等写法。
pub fn mask(text: &str, code: &str) -> String {
    if code.is_empty() {
        return text.into();
    }
    let pattern = code
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| format!("[{c}{}]", char::from_u32(c as u32 + 0xfee0).unwrap_or(c)))
        .collect::<Vec<_>>()
        .join(r"[\s\-\u{ff0d}\u{200b}\u{200c}\u{200d}\u{feff}\u{2060}]*");
    if pattern.is_empty() {
        return text.replace(code, "••••••");
    }
    regex::RegexBuilder::new(&pattern)
        .case_insensitive(true)
        .build()
        .map(|r| r.replace_all(text, "••••••").into_owned())
        .unwrap_or_else(|_| text.replace(code, "••••••"))
}

impl Privacy {
    /// 每帧对当前邮件条目调用；只在首次看到时记录时间。
    pub fn observe(&mut self, item: &Item, now: f64) {
        if item.code.is_some() {
            self.seen.entry(key(item)).or_insert(now);
        }
    }
    /// 是否应遮罩：超过设定秒数，或用户点开已过期。
    pub fn hidden(&self, item: &Item, now: f64, fallback: u32) -> bool {
        if item.code.is_none() {
            return false;
        }
        let k = key(item);
        if self.revealed.get(&k).is_some_and(|until| now < *until) {
            return false;
        }
        let Some(&at) = self.seen.get(&k) else { return false; };
        now - at >= fallback.clamp(15, 300) as f64
    }
    /// 手动点开，临时显示 15 秒。
    pub fn reveal(&mut self, item: &Item, now: f64) {
        self.revealed.insert(key(item), now + 15.0);
    }
    /// 清掉已经不存在的邮件条目，避免 map 无限增长。
    pub fn retain(&mut self, items: &[Item]) {
        let keys: HashSet<_> = items.iter().filter(|i| i.code.is_some()).map(key).collect();
        self.seen.retain(|k, _| keys.contains(k));
        self.revealed.retain(|k, _| keys.contains(k));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(code: &str) -> Item {
        Item {
            account: "test@example.com".into(),
            from: "Example".into(),
            subject: format!("验证码 {code}"),
            time: String::new(),
            code: Some(code.into()),
            summary: format!("验证码 {code}，请勿泄露"),
            links: vec![],
            meeting: None,
            uid: 1,
        }
    }

    #[test]
    fn hides_only_after_the_configured_seconds() {
        let i = item("123456");
        let mut p = Privacy::default();
        p.observe(&i, 100.0);
        assert!(!p.hidden(&i, 159.9, 60));
        assert!(p.hidden(&i, 160.0, 60));
    }

    #[test]
    fn manual_reveal_is_temporary() {
        let i = item("123456");
        let mut p = Privacy::default();
        p.observe(&i, 0.0);
        assert!(p.hidden(&i, 70.0, 60));
        p.reveal(&i, 70.0);
        assert!(!p.hidden(&i, 80.0, 60));
        assert!(p.hidden(&i, 86.0, 60));
    }

    #[test]
    fn masking_covers_formatted_variants() {
        for (text, code) in [
            ("验证码 １２３４５６", "123456"),
            ("验证码 12\u{200b}3456", "123456"),
            ("验证码 123 456", "123456"),
            ("验证码 ＡＢＣ－１２３", "ABC-123"),
        ] {
            assert_eq!(mask(text, code), "验证码 ••••••", "归一化验证码必须遮住原文");
        }
    }

    #[test]
    fn no_code_never_hidden() {
        let mut i = item("123456");
        i.code = None;
        let mut p = Privacy::default();
        p.observe(&i, 0.0);
        assert!(!p.hidden(&i, 10_000.0, 15));
    }
}
