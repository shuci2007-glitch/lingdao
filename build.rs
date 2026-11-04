//! 把岛仔图标作为 RT_GROUP_ICON #1 编进 exe，供 LoadImageW(hInst, 1, ...) 取用。
//! 不引入额外依赖：直接调用 Windows SDK 的 rc.exe，把 .res 交给链接器。

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn find_rc() -> Option<PathBuf> {
    // 候选根目录：覆盖 C:/D:/E: 盘的标准安装位置与常见摆放。
    let mut roots: Vec<PathBuf> = vec![
        r"C:\Program Files (x86)\Windows Kits\10\bin".into(),
        r"C:\Program Files\Windows Kits\10\bin".into(),
        r"D:\Program Files (x86)\Windows Kits\10\bin".into(),
        r"D:\Program Files\Windows Kits\10\bin".into(),
        r"D:\Windows Kits\10\bin".into(),
        r"E:\Program Files (x86)\Windows Kits\10\bin".into(),
        r"E:\Program Files\Windows Kits\10\bin".into(),
        r"E:\Windows Kits\10\bin".into(),
    ];
    // VS/SDK 注入的环境变量优先（Developer Command Prompt / vcvarsall）。
    if let Ok(sdk) = env::var("WindowsSdkDir") {
        roots.insert(0, PathBuf::from(sdk).join("bin"));
    }
    let mut found: Vec<PathBuf> = Vec::new();
    for root in roots {
        let Ok(entries) = fs::read_dir(&root) else { continue };
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            for arch in ["x64", "x86", "arm64"] {
                let rc = dir.join(arch).join("rc.exe");
                if rc.exists() {
                    found.push(rc);
                }
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
        println!("cargo:warning=rc.exe 未找到，跳过图标资源");
        return;
    };
    let status = Command::new(&rc)
        .arg("/nologo")
        .arg("/fo")
        .arg(&out)
        .arg("app.rc")
        .status();
    if !status.map(|s| s.success()).unwrap_or(false) {
        println!("cargo:warning=rc.exe 编译 app.rc 失败");
        return;
    }
    println!("cargo:rustc-link-arg={}", out.display());
}
