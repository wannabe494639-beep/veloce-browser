const COMMANDS: &[&str] = &[
    "createTab",
    "activateTab",
    "closeTab",
    "navigate",
    "historyMove",
    "reload",
    "setZoom",
    "evalScript",
    "hasTab",
    "setTrackingProtection",
    "setLayout",
    "setOverlay",
    "openDownload",
    "sharePage",
    "requestDefaultBrowser",
    "consumeExternalUrl",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .build();
}
