//! 启动错误与 Rust 异常保存在本机，不自动上传账号或应用数据。

use std::path::{Path, PathBuf};

fn write_report(directory: &Path, title: &str, details: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(directory)?;
    let path = directory.join("last-error.log");
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
    std::fs::write(&path, format!(
        "灵岛 {}\n系统：{} / {}\n时间（Unix 秒）：{timestamp}\n{title}\n{details}\n",
        env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH,
    ))?;
    Ok(path)
}

fn message(directory: &Path, title: &str, details: &str) -> String {
    match write_report(directory, title, details) {
        Ok(path) => format!("{title}\n\n请将以下日志发给开发者，帮助定位问题：\n{}", path.display()),
        Err(error) => format!("{title}\n\n{details}\n\n日志无法保存：{error}\n请截图反馈。"),
    }
}

pub fn report(title: &str, details: &str) {
    let text = message(&lingdao::model::config_dir().join("diagnostics"), title, details);
    #[cfg(windows)]
    {
        use windows::core::{w, PCWSTR};
        use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
        let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
        unsafe { MessageBoxW(None, PCWSTR(wide.as_ptr()), w!("灵岛"), MB_OK | MB_ICONERROR); }
    }
    #[cfg(not(windows))]
    eprintln!("{text}");
}

pub fn install() {
    use std::sync::atomic::{AtomicBool, Ordering};
    // 后台线程可能反复 panic（例如网络连接状态错乱时每轮重连都踩一次）。
    // 每 panic 一次就弹一个模态框，会迅速把桌面堆满、表现为整机卡死，
    // 所以只在第一次完整提示，之后仅落盘，不再弹窗。
    static REPORTED: AtomicBool = AtomicBool::new(false);
    std::panic::set_hook(Box::new(|info| {
        // 不写入 panic 的数据载荷，避免把邮件、凭据或登录信息带进日志。
        let location = info.location().map(|p| format!("{}:{}:{}", p.file(), p.line(), p.column()))
            .unwrap_or_else(|| "未知".into());
        let details = format!(
            "代码位置：{location}\n调用栈：\n{}", std::backtrace::Backtrace::force_capture(),
        );
        if REPORTED.swap(true, Ordering::SeqCst) {
            let _ = write_report(
                &lingdao::model::config_dir().join("diagnostics"),
                "灵岛发生异常（重复异常，仅记录不再弹窗）",
                &details,
            );
        } else {
            report("灵岛发生异常，请重新启动。", &details);
        }
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn directory(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("daozai-diagnostics-{}-{name}", std::process::id()))
    }

    #[test]
    fn failure_creates_a_report_and_exposes_its_location() {
        let directory = directory("report");
        let text = message(&directory, "启动失败", "OpenGL 初始化失败");
        let path = directory.join("last-error.log");
        assert!(text.contains(&path.display().to_string()));
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains(env!("CARGO_PKG_VERSION")));
        assert!(content.contains("OpenGL 初始化失败"));
        // 重复失败只保留最后一份，避免常驻程序无限累积日志。
        write_report(&directory, "第二次失败", "新的错误").unwrap();
        assert!(!std::fs::read_to_string(&path).unwrap().contains("OpenGL"));
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn unwritable_log_keeps_the_error_visible() {
        let path = directory("blocked");
        std::fs::write(&path, b"test").unwrap();
        let text = message(&path, "启动失败", "OpenGL 初始化失败");
        assert!(text.contains("OpenGL 初始化失败"));
        assert!(text.contains("日志无法保存"));
        std::fs::remove_file(path).unwrap();
    }
}
