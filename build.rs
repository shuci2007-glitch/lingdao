//! 把岛仔图标（RT_GROUP_ICON #1）与版本信息编进 exe，供 LoadImageW(hInst, 1, ...) 及
//! 资源管理器属性页取用。
//!
//! 依赖 Windows SDK 的 rc.exe 编译 app.rc（app.rc 内含图标 + VERSIONINFO）。若环境无
//! rc.exe（如本机未安装 Windows SDK），则跳过资源编译——此时 exe 不带图标/版本资源，
//! 但程序功能不受影响。需在正式 CI（含 Windows SDK）中构建以获得完整资源。

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn find_rc() -> Option<PathBuf> {
    let roots = [
        r"C:\Program Files (x86)\Windows Kits\10\bin",
        r"C:\Program Files\Windows Kits\10\bin",
    ];
    let mut found: Vec<PathBuf> = Vec::new();
    for root in roots {
        let Ok(entries) = fs::read_dir(root) else { continue };
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let rc = dir.join("x64").join("rc.exe");
            if rc.exists() {
                found.push(rc);
            }
        }
    }
    found.sort();
    found.pop()
}

fn main() {
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=assets/daozai.ico");
    println!("cargo:rerun-if-changed=build.rs");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("daozai.res");
    let Some(rc) = find_rc() else {
        println!("cargo:warning=rc.exe 未找到（未安装 Windows SDK），跳过图标/版本资源编译；exe 将不带资源");
        return;
    };
    let status = Command::new(&rc)
        .arg("/nologo")
        .arg("/fo")
        .arg(&out)
        .arg("app.rc")
        .status();
    if status.map(|s| s.success()).unwrap_or(false) {
        println!("cargo:rustc-link-arg={}", out.display());
    } else {
        println!("cargo:warning=rc.exe 编译 app.rc 失败，跳过资源编译");
    }
}
