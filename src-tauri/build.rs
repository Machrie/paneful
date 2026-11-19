fn main() {
    // An app manifest puts every app command under the ACL: the host
    // capability lists what the local UI may call, the panel capability
    // the three commands x.com pages may call.
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "refresh_layout",
            "set_ratios",
            "commit_layout",
            "reload_panel",
            "add_account",
            "remove_account",
            "panel_focused",
            "shortcut",
            "panel_keys",
            "set_key",
            "set_recording",
            "pick_save_dir",
            "reset_save_dir",
            "open_save_dir",
            "capture_begin",
            "capture_shot",
            "capture_drop",
            "capture_finish",
            "capture_close",
            "capture_slice",
            "capture_export",
        ]),
    ))
    .expect("failed to run tauri-build");
}
