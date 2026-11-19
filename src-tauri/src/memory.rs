//! Memory per panel, for the settings screen.
//!
//! - Windows: WebView2 `GetProcessExtendedInfos` lists every process of the
//!   shared WebView2 runtime with the frames each renderer runs. A renderer
//!   whose frames all belong to one panel (by the main frame id,
//!   `ICoreWebView2_20::FrameId`) counts for that panel; the browser, GPU,
//!   utility processes and renderers with no / mixed frames count as
//!   "shared". Size = private working set (the Task Manager "Memory" column).
//! - macOS: each panel's WebContent process id (`_webProcessIdentifier`,
//!   private WKWebView API, checked with respondsToSelector first) and its
//!   physical footprint (Activity Monitor "Memory"). WebKit's GPU and
//!   Networking processes aren't reachable this way, so there is no
//!   "shared" figure.
//!
//! The main thread only collects ids; sizes are read off it. No lock is held
//! while touching a webview.

use serde::Serialize;
use std::collections::BTreeMap;
use tauri::{AppHandle, Manager, Runtime};

#[derive(Serialize, Clone, Debug)]
pub struct PanelMem {
    pub id: String,
    pub label: String,
    pub color: String,
    /// Bytes; None: no process found for this panel.
    pub bytes: Option<u64>,
    pub procs: usize,
    /// Last dead panel recovery (health.rs).
    pub last: Option<crate::health::Last>,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Report {
    pub panels: Vec<PanelMem>,
    /// Processes not tied to one panel (Windows only).
    pub shared: Option<u64>,
    /// The app's own UI webview (top bar, settings).
    pub host: Option<u64>,
    /// The Paneful process itself.
    pub app: Option<u64>,
    pub total: Option<u64>,
    /// What the numbers are, for the hint line.
    pub method: String,
    pub error: Option<String>,
}

/// Who a process counts for.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Owner {
    Panel(String),
    Host,
    Shared,
}

/// One process and the webviews (panel labels / HOST) it serves. Empty or
/// more than one: shared.
#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
pub fn owner_of(owners: &[String]) -> Owner {
    let mut o: Vec<&String> = owners.iter().collect();
    o.sort();
    o.dedup();
    match o.as_slice() {
        [one] if one.as_str() == crate::HOST => Owner::Host,
        [one] => Owner::Panel((*one).clone()),
        _ => Owner::Shared,
    }
}

/// Sum sizes per owner. A pid listed twice counts once (for the first
/// owner it was seen with); unreadable sizes are left out.
pub fn sum(procs: &[(u32, Owner, Option<u64>)]) -> BTreeMap<Owner, (u64, usize)> {
    let mut seen = std::collections::HashSet::new();
    let mut out: BTreeMap<Owner, (u64, usize)> = BTreeMap::new();
    for (pid, owner, bytes) in procs {
        if !seen.insert(*pid) {
            continue;
        }
        if let Some(b) = bytes {
            let e = out.entry(owner.clone()).or_default();
            e.0 += b;
            e.1 += 1;
        }
    }
    out
}

/// Measure now. Ok even when some parts fail; see `Report::error`.
pub async fn measure<R: Runtime>(app: &AppHandle<R>) -> Report {
    let accounts: Vec<(String, String, String, String)> = {
        let state = app.state::<crate::AppState>();
        let s = state.settings.lock().unwrap();
        s.accounts
            .iter()
            .map(|a| (crate::panel_label(a), a.id.to_string(), a.label.clone(), a.color.clone()))
            .collect()
    };
    let labels: Vec<String> = accounts.iter().map(|a| a.0.clone()).collect();
    let (procs, method, error) = match collect(app, &labels).await {
        Ok((p, m)) => (p, m, None),
        Err((m, e)) => (vec![], m, Some(e)),
    };
    let sums = sum(&procs);
    let get = |o: &Owner| sums.get(o).copied();
    let app_bytes = own_size();
    let panels: Vec<PanelMem> = accounts
        .iter()
        .map(|(label, id, name, color)| {
            let s = get(&Owner::Panel(label.clone()));
            PanelMem {
                id: id.clone(),
                label: name.clone(),
                color: color.clone(),
                bytes: s.map(|s| s.0),
                procs: s.map_or(0, |s| s.1),
                last: crate::health::last(label),
            }
        })
        .collect();
    let shared = get(&Owner::Shared).map(|s| s.0);
    let host = get(&Owner::Host).map(|s| s.0);
    let total = if procs.is_empty() {
        None
    } else {
        Some(sums.values().map(|s| s.0).sum::<u64>() + app_bytes.unwrap_or(0))
    };
    Report { panels, shared, host, app: app_bytes, total, method, error }
}

type Collected = Result<(Vec<(u32, Owner, Option<u64>)>, String), (String, String)>;

const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Wait off the main thread for what the main thread sends back.
#[allow(dead_code)]
async fn recv<T: Send + 'static>(rx: std::sync::mpsc::Receiver<T>) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(move || rx.recv_timeout(TIMEOUT).map_err(|_| "시간 초과".to_string()))
        .await
        .map_err(|e| e.to_string())?
}

/// Up to `n` answers from the main thread; whatever came within TIMEOUT.
#[allow(dead_code)]
async fn recv_n<T: Send + 'static>(rx: std::sync::mpsc::Receiver<T>, n: usize) -> Result<Vec<T>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let deadline = std::time::Instant::now() + TIMEOUT;
        let mut v = vec![];
        while v.len() < n {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            match rx.recv_timeout(left) {
                Ok(x) => v.push(x),
                Err(_) => break,
            }
        }
        v
    })
    .await
    .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------- Windows

#[cfg(windows)]
const METHOD: &str = "개인 작업 집합 기준 (작업 관리자 '메모리' 열). GPU·네트워크 등 WebView2 공용 프로세스는 '공용'으로 따로 셈.";

#[cfg(windows)]
async fn collect<R: Runtime>(app: &AppHandle<R>, labels: &[String]) -> Collected {
    use std::sync::mpsc;
    use webview2_com::{
        GetProcessExtendedInfosCompletedHandler,
        Microsoft::Web::WebView2::Win32::*,
    };
    use windows::core::Interface;

    let m = || METHOD.to_string();
    // 1. main frame id of every panel and of the host UI.
    let (tx, rx) = mpsc::channel::<(String, Option<u32>)>();
    let mut asked = 0;
    for label in labels.iter().map(String::as_str).chain([crate::HOST]) {
        let Some(wv) = app.get_webview(label) else { continue };
        let tx = tx.clone();
        let label = label.to_string();
        let sent = wv.with_webview(move |pw| unsafe {
            let id = pw
                .controller()
                .CoreWebView2()
                .ok()
                .and_then(|c| c.cast::<ICoreWebView2_20>().ok())
                .and_then(|c| {
                    let mut id = 0u32;
                    c.FrameId(&mut id).ok().map(|_| id)
                })
                .filter(|id| *id != 0);
            let _ = tx.send((label, id));
        });
        if sent.is_ok() {
            asked += 1;
        }
    }
    drop(tx);
    let frames: Vec<(u32, String)> = recv_n(rx, asked)
        .await
        .map_err(|e| (m(), e))?
        .into_iter()
        .filter_map(|(label, id)| id.map(|id| (id, label)))
        .collect();

    // 2. every process with the main frames it runs, through the host
    //    webview (same WebView2 browser process as the panels).
    let host = app.get_webview(crate::HOST).ok_or((m(), "앱 화면 없음".to_string()))?;
    // (pid, kind, main frame ids)
    let (tx, rx) = mpsc::channel::<Result<Vec<(u32, i32, Vec<u32>)>, String>>();
    host.with_webview(move |pw| unsafe {
        let start = (|| -> windows::core::Result<()> {
            let core = pw.controller().CoreWebView2()?;
            let env = core.cast::<ICoreWebView2_2>()?.Environment()?;
            let env13 = env.cast::<ICoreWebView2Environment13>()?;
            let tx2 = tx.clone();
            let handler = GetProcessExtendedInfosCompletedHandler::create(Box::new(move |r, list| {
                let out = r.and_then(|_| read_infos(list)).map_err(|e| e.to_string());
                let _ = tx2.send(out);
                Ok(())
            }));
            env13.GetProcessExtendedInfos(&handler)
        })();
        if let Err(e) = start {
            let _ = tx.send(Err(format!("WebView2 프로세스 목록을 못 받음 (런타임이 오래됐을 수 있음): {e}")));
        }
    })
    .map_err(|e| (m(), e.to_string()))?;
    let infos = recv(rx).await.and_then(|r| r).map_err(|e| (m(), e))?;

    // 3. sizes, off the main thread.
    let out = tauri::async_runtime::spawn_blocking(move || {
        infos
            .into_iter()
            .map(|(pid, kind, mains)| {
                let owner = if kind == COREWEBVIEW2_PROCESS_KIND_RENDERER.0 {
                    let owners: Vec<String> = mains
                        .iter()
                        .filter_map(|f| frames.iter().find(|(id, _)| id == f).map(|(_, l)| l.clone()))
                        .collect();
                    // A renderer running frames we can't place is shared too.
                    if owners.len() == mains.len() {
                        owner_of(&owners)
                    } else {
                        Owner::Shared
                    }
                } else {
                    Owner::Shared
                };
                (pid, owner, private_ws(pid))
            })
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|e| (m(), e.to_string()))?;
    Ok((out, m()))
}

/// Main thread, inside the completion handler: pid, kind and the main frame
/// id of every frame each process runs.
#[cfg(windows)]
unsafe fn read_infos(
    list: Option<webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2ProcessExtendedInfoCollection>,
) -> windows::core::Result<Vec<(u32, i32, Vec<u32>)>> {
    use webview2_com::Microsoft::Web::WebView2::Win32::*;
    let Some(list) = list else { return Ok(vec![]) };
    let mut n = 0u32;
    list.Count(&mut n)?;
    let mut out = vec![];
    for i in 0..n {
        let Ok(ext) = list.GetValueAtIndex(i) else { continue };
        let Ok(info) = ext.ProcessInfo() else { continue };
        let mut pid = 0i32;
        let mut kind = COREWEBVIEW2_PROCESS_KIND::default();
        let _ = info.ProcessId(&mut pid);
        let _ = info.Kind(&mut kind);
        let mut mains = vec![];
        if let Ok(it) = ext.AssociatedFrameInfos().and_then(|c| c.GetIterator()) {
            let mut has = windows::core::BOOL::default();
            let _ = it.HasCurrent(&mut has);
            while has.as_bool() {
                if let Ok(f) = it.GetCurrent() {
                    mains.push(main_frame_of(f).unwrap_or(0));
                }
                if it.MoveNext(&mut has).is_err() {
                    break;
                }
            }
        }
        if pid > 0 {
            out.push((pid as u32, kind.0, mains));
        }
    }
    Ok(out)
}

/// Walk up to the main frame and return its id (0 if it can't be found).
#[cfg(windows)]
unsafe fn main_frame_of(
    mut f: webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2FrameInfo,
) -> Option<u32> {
    use webview2_com::Microsoft::Web::WebView2::Win32::*;
    use windows::core::Interface;
    for _ in 0..64 {
        let f2 = f.cast::<ICoreWebView2FrameInfo2>().ok()?;
        let mut kind = COREWEBVIEW2_FRAME_KIND::default();
        f2.FrameKind(&mut kind).ok()?;
        if kind == COREWEBVIEW2_FRAME_KIND_MAIN_FRAME {
            let mut id = 0u32;
            f2.FrameId(&mut id).ok()?;
            return Some(id);
        }
        f = f2.ParentFrameInfo().ok()?;
    }
    None
}

/// Private working set of `pid`, falling back to private bytes (commit) on
/// Windows builds without PROCESS_MEMORY_COUNTERS_EX2.
#[cfg(windows)]
fn private_ws(pid: u32) -> Option<u64> {
    use windows::Win32::{
        Foundation::CloseHandle,
        System::{
            ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX, PROCESS_MEMORY_COUNTERS_EX2},
            Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
        },
    };
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut c2 = PROCESS_MEMORY_COUNTERS_EX2 { cb: size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32, ..Default::default() };
        let out = if GetProcessMemoryInfo(h, &mut c2 as *mut _ as *mut PROCESS_MEMORY_COUNTERS, c2.cb).is_ok() {
            Some(c2.PrivateWorkingSetSize as u64)
        } else {
            let mut c = PROCESS_MEMORY_COUNTERS_EX { cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32, ..Default::default() };
            GetProcessMemoryInfo(h, &mut c as *mut _ as *mut PROCESS_MEMORY_COUNTERS, c.cb)
                .ok()
                .map(|_| c.PrivateUsage as u64)
        };
        let _ = CloseHandle(h);
        out
    }
}

#[cfg(windows)]
fn own_size() -> Option<u64> {
    private_ws(std::process::id())
}

// ---------------------------------------------------------------- macOS

#[cfg(target_os = "macos")]
const METHOD: &str = "물리 메모리 기준 (활성 상태 보기 '메모리' 열). 패널마다 웹 콘텐츠 프로세스만 셈. WebKit GPU·네트워크 프로세스는 못 재서 빠짐.";

#[cfg(target_os = "macos")]
async fn collect<R: Runtime>(app: &AppHandle<R>, labels: &[String]) -> Collected {
    use objc2::{msg_send, sel};
    use objc2_foundation::NSObjectProtocol;
    use objc2_web_kit::WKWebView;
    use std::sync::mpsc;

    let m = || METHOD.to_string();
    let (tx, rx) = mpsc::channel::<(String, Option<u32>)>();
    let mut asked = 0usize;
    for label in labels.iter().map(String::as_str).chain([crate::HOST]) {
        let Some(wv) = app.get_webview(label) else { continue };
        let tx = tx.clone();
        let label = label.to_string();
        let sent = wv.with_webview(move |pw| unsafe {
            let view: &WKWebView = &*(pw.inner() as *const WKWebView);
            let pid = if view.respondsToSelector(sel!(_webProcessIdentifier)) {
                let pid: i32 = msg_send![view, _webProcessIdentifier];
                (pid > 0).then_some(pid as u32)
            } else {
                None
            };
            let _ = tx.send((label, pid));
        });
        if sent.is_ok() {
            asked += 1;
        }
    }
    drop(tx);
    let got = recv_n(rx, asked).await.map_err(|e| (m(), e))?;
    if got.iter().all(|(_, p)| p.is_none()) {
        return Err((m(), "웹 콘텐츠 프로세스 번호를 못 얻음 (이 macOS의 WebKit에서 막힘)".into()));
    }
    // Same pid for several webviews: shared.
    let mut by_pid: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    for (label, pid) in got {
        if let Some(pid) = pid {
            by_pid.entry(pid).or_default().push(label);
        }
    }
    let out = by_pid
        .into_iter()
        .map(|(pid, owners)| (pid, owner_of(&owners), footprint(pid)))
        .collect();
    Ok((out, m()))
}

#[cfg(target_os = "macos")]
fn footprint(pid: u32) -> Option<u64> {
    let mut info: libc::rusage_info_v2 = unsafe { std::mem::zeroed() };
    let r = unsafe {
        libc::proc_pid_rusage(pid as i32, libc::RUSAGE_INFO_V2, &mut info as *mut _ as *mut libc::rusage_info_t)
    };
    (r == 0).then_some(info.ri_phys_footprint)
}

#[cfg(target_os = "macos")]
fn own_size() -> Option<u64> {
    footprint(std::process::id())
}

// ---------------------------------------------------------------- other

#[cfg(not(any(windows, target_os = "macos")))]
async fn collect<R: Runtime>(_app: &AppHandle<R>, _labels: &[String]) -> Collected {
    Err((String::new(), "이 OS에서는 지원 안 함".into()))
}

#[cfg(not(any(windows, target_os = "macos")))]
fn own_size() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn owner_rules() {
        assert_eq!(owner_of(&s(&["panel-a"])), Owner::Panel("panel-a".into()));
        assert_eq!(owner_of(&s(&["panel-a", "panel-a"])), Owner::Panel("panel-a".into()));
        assert_eq!(owner_of(&s(&[crate::HOST])), Owner::Host);
        assert_eq!(owner_of(&s(&["panel-a", "panel-b"])), Owner::Shared);
        assert_eq!(owner_of(&[]), Owner::Shared);
    }

    #[test]
    fn sums_once_per_pid_and_skips_unknown() {
        let a = Owner::Panel("panel-a".into());
        let procs = vec![
            (1, a.clone(), Some(100)),
            (2, a.clone(), Some(50)),
            (2, Owner::Shared, Some(50)), // same pid again: ignored
            (3, Owner::Shared, None),     // unreadable: left out
            (4, Owner::Host, Some(7)),
        ];
        let m = sum(&procs);
        assert_eq!(m.get(&a), Some(&(150, 2)));
        assert_eq!(m.get(&Owner::Host), Some(&(7, 1)));
        assert_eq!(m.get(&Owner::Shared), None);
    }
}
