//! 把灵岛图标作为 RT_GROUP_ICON #1 编进 exe，供 LoadImageW(hInst, 1, ...) 取用。
//! 不引入额外依赖：直接调用 Windows SDK 的 rc.exe，把 .res 交给链接器。
//! 版本号字段从 Cargo.toml 的 CARGO_PKG_VERSION 注入，避免 app.rc 手动改漏导致
//! 数值版本（FILEVERSION）与字符串版本、Cargo 版本三者漂移。

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

/// 把 "0.6.54" 这样的语义版本号转成 RC 的 "0,6,54,0" 形式。
fn file_version(pkg: &str) -> String {
    let parts: Vec<&str> = pkg.split('.').collect();
    if parts.len() == 3 {
        format!("{},{},{},0", parts[0], parts[1], parts[2])
    } else {
        pkg.replace('.', ",")
    }
}

/// 读取 app.rc 模板，把版本相关字段替换成当前 Cargo 版本后写到 out_path。
fn generate_rc(out_path: &PathBuf, pkg: &str) {
    let template = fs::read_to_string("app.rc").unwrap_or_else(|e| panic!("读取 app.rc 失败：{e}"));
    let filever = file_version(pkg);
    let mut out = String::with_capacity(template.len());
    for line in template.lines() {
        let trimmed = line.trim_start();
        let indent = &line[..line.len() - trimmed.len()];
        if trimmed.starts_with("FILEVERSION ") {
            out.push_str(&format!("{indent}FILEVERSION {filever}\n"));
        } else if trimmed.starts_with("PRODUCTVERSION ") {
            out.push_str(&format!("{indent}PRODUCTVERSION {filever}\n"));
        } else if trimmed.starts_with("VALUE \"FileVersion\"") {
            out.push_str(&format!("{indent}VALUE \"FileVersion\", \"{pkg}\"\n"));
        } else if trimmed.starts_with("VALUE \"ProductVersion\"") {
            out.push_str(&format!("{indent}VALUE \"ProductVersion\", \"{pkg}\"\n"));
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    fs::write(out_path, out).unwrap_or_else(|e| panic!("写入生成用 app.rc 失败：{e}"));
}

fn main() {
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=assets/daozai.ico");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let pkg = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let generated = out_dir.join("app_generated.rc");
    generate_rc(&generated, &pkg);
    let out = out_dir.join("lingdao.res");
    let Some(rc) = find_rc() else {
        println!("cargo:warning=rc.exe 未找到，跳过图标资源");
        return;
    };
    let status = Command::new(&rc)
        .arg("/nologo")
        .arg("/fo")
        .arg(&out)
        .arg(&generated)
        .status();
    if !status.map(|s| s.success()).unwrap_or(false) {
        println!("cargo:warning=rc.exe 编译 app.rc 失败");
        return;
    }
    println!("cargo:rustc-link-arg={}", out.display());
}
