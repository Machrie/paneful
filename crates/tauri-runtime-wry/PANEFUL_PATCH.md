# Vendored tauri-runtime-wry 2.12.5 (patched)

Unmodified copy of `tauri-runtime-wry` 2.12.5 from crates.io (MIT OR
Apache-2.0, see LICENSE-MIT / LICENSE-APACHE-2.0, copyright Tauri
Programme within The Commons Conservancy), with one change in `src/lib.rs`
marked `paneful patch`:

On Windows, `WebviewAttributes::data_store_identifier` is converted to a
32-char lowercase hex string and passed to wry's
`WebViewBuilderExtWindows::with_profile_name`, so each account gets its own
WebView2 profile inside one shared runtime. Upstream equivalent:
tauri-apps/tauri#16054.

Wired in through `[patch.crates-io]` in the workspace `Cargo.toml`.
Delete this directory and that patch entry once upstream ships it.
