#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod ui;
use qilight::{
    config,
    control::{Action, Engine},
    message, Result,
};
use std::{path::PathBuf, sync::Arc, time::Duration};

struct Options {
    config: PathBuf,
    check: bool,
    qr_check: bool,
    demo: bool,
    smoke: bool,
    screenshot: Option<PathBuf>,
    report: Option<PathBuf>,
    action: Action,
}
fn options() -> Result<Options> {
    let executable = std::env::current_exe()?;
    let directory = executable
        .parent()
        .ok_or_else(|| message("无法找到程序目录。"))?;
    let default = config_path(directory);
    let mut options = Options {
        config: default,
        check: false,
        qr_check: false,
        demo: false,
        smoke: false,
        screenshot: None,
        report: None,
        action: Action::Refresh,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => {
                options.config = PathBuf::from(
                    args.next()
                        .ok_or_else(|| message("--config 需要文件路径。"))?,
                )
            }
            "--check" => options.check = true,
            "--qr-check" => options.qr_check = true,
            "--demo" => options.demo = true,
            "--ui-check" => options.smoke = true,
            "--screenshot" => {
                options.screenshot = Some(PathBuf::from(
                    args.next()
                        .ok_or_else(|| message("--screenshot 需要文件路径。"))?,
                ))
            }
            "--report" => {
                options.report = Some(PathBuf::from(
                    args.next()
                        .ok_or_else(|| message("--report 需要文件路径。"))?,
                ))
            }
            "--on" => options.action = Action::Power(true),
            "--off" => options.action = Action::Power(false),
            _ => {
                return Err(message(
                    "参数无效。支持 --check、--config、--on、--off、--demo、--ui-check。",
                ))
            }
        }
    }
    Ok(options)
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
fn run() -> Result<()> {
    let options = options()?;
    if options.qr_check {
        let mut client = qilight::cloud::CloudSetup::new(
            "cn",
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )?;
        let qr = client.create_qr()?;
        let image = image::load_from_memory(&qr.bytes).map_err(|_| message("二维码图片无效。"))?;
        let result = serde_json::json!({"qr_image_valid":true,"width":image.width(),"height":image.height(),"expires_in":qr.expires_in});
        if let Some(path) = options.report {
            std::fs::write(path, serde_json::to_vec_pretty(&result)?)?;
        }
        println!("{result}");
        return Ok(());
    }
    if options.check {
        let devices = config::load(&options.config)?;
        let count = devices.len();
        let engine = Engine::new(Arc::new(|| {}));
        for device in devices {
            engine.submit(1, device, Action::Refresh);
        }
        let mut results = Vec::new();
        for _ in 0..count {
            let completion = engine
                .results
                .recv_timeout(Duration::from_secs(30))
                .map_err(|_| message("设备检查超时。"))?;
            let r = completion.outcome;
            results.push(serde_json::json!({"ip":r.ip,"state":r.state,"error":r.error}));
        }
        let ok = count > 0 && results.iter().all(|r| r["error"].is_null());
        let text = serde_json::to_string_pretty(&results)?;
        if let Some(path) = options.report {
            std::fs::write(path, &text)?;
        }
        println!("{text}");
        if !ok {
            return Err(message("部分灯具未通过连接检查。"));
        }
        return Ok(());
    }
    let _instance = if !options.demo && !options.smoke {
        Some(Instance::acquire()?)
    } else {
        None
    };
    let icon = image::load_from_memory(include_bytes!("../assets/lamp.ico"))
        .ok()
        .map(|im| {
            let image = im.to_rgba8();
            eframe::egui::IconData {
                width: image.width(),
                height: image.height(),
                rgba: image.into_raw(),
            }
        });
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_inner_size(if options.smoke {
            [900.0, 620.0]
        } else {
            [1080.0, 700.0]
        })
        .with_min_inner_size([900.0, 620.0]);
    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }
    let native =
        eframe::NativeOptions {
            viewport,
            renderer: eframe::Renderer::Glow,
            persist_window: !options.demo && !options.smoke,
            persistence_path: Some(options.config.with_extension(
                if options.demo || options.smoke {
                    "preview.ron"
                } else {
                    "window.ron"
                },
            )),
            ..Default::default()
        };
    eframe::run_native(
        ui::TITLE,
        native,
        Box::new(move |cc| {
            Ok(Box::new(ui::App::new(
                &cc.egui_ctx,
                options.config,
                options.demo,
                options.smoke,
                options.screenshot,
                options.action,
            )))
        }),
    )
    .map_err(|_| message("桌面窗口无法启动，请检查显卡驱动与桌面会话。"))?;
    Ok(())
}

fn config_path(directory: &std::path::Path) -> PathBuf {
    let current = directory.join("data/devices.bin");
    let legacy = directory.join("devices.bin");
    if !current.exists() && legacy.exists() {
        legacy
    } else {
        current
    }
}

#[cfg(windows)]
struct Instance(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
impl Instance {
    fn acquire() -> Result<Self> {
        use windows_sys::Win32::{
            Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS},
            System::Threading::CreateMutexW,
            UI::WindowsAndMessaging::{FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE},
        };
        let name: Vec<u16> = "Local\\QiLight".encode_utf16().chain(Some(0)).collect();
        unsafe {
            let handle = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
            if handle.is_null() {
                return Err(std::io::Error::last_os_error().into());
            }
            if GetLastError() == ERROR_ALREADY_EXISTS {
                let title: Vec<u16> = ui::TITLE.encode_utf16().chain(Some(0)).collect();
                let window = FindWindowW(std::ptr::null(), title.as_ptr());
                if !window.is_null() {
                    ShowWindow(window, SW_RESTORE);
                    SetForegroundWindow(window);
                }
                CloseHandle(handle);
                std::process::exit(0);
            }
            Ok(Self(handle))
        }
    }
}
#[cfg(windows)]
impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
#[cfg(not(windows))]
struct Instance;
#[cfg(not(windows))]
impl Instance {
    fn acquire() -> Result<Self> {
        Ok(Self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_organized_data_and_still_accepts_legacy_config() {
        let directory =
            tempfile::tempdir_in(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target"))
                .unwrap();
        let root = directory.path();
        assert_eq!(config_path(root), root.join("data/devices.bin"));
        std::fs::write(root.join("devices.bin"), b"legacy").unwrap();
        assert_eq!(config_path(root), root.join("devices.bin"));
        std::fs::create_dir(root.join("data")).unwrap();
        std::fs::write(root.join("data/devices.bin"), b"current").unwrap();
        assert_eq!(config_path(root), root.join("data/devices.bin"));
    }
}
