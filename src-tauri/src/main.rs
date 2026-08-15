// 发布版在 Windows 上不创建额外的控制台窗口。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// 启动桌面应用。
fn main() {
    quailcard_lib::run()
}
