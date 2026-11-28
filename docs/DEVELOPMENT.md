# 개발

Tauri 2 (Rust). 프런트엔드는 빌드 단계 없는 정적 파일(`ui/`)이다.

## 필요한 것

- Rust stable
- Node 22: 설치 파일을 만드는 Tauri CLI(`npx @tauri-apps/cli`)를 돌릴 때만 쓴다. 앱 자체엔 Node가 들어가지 않는다
- Windows: WebView2 런타임, macOS: Xcode Command Line Tools

## 명령

```sh
cargo run -p paneful                  # 개발 실행
cargo test -p paneful --lib           # 단위 테스트
npx @tauri-apps/cli@2 build           # 설치 파일 (Windows: NSIS .exe, macOS: .dmg)
```

## 구조

- 한 창 = 호스트 UI 웹뷰(상단 바, 패널 사이 틈) + 계정별 x.com 자식 웹뷰 N개 (`tauri` `unstable` 피처)
- 계정 = UUID = 데이터 저장소
  - macOS 14+: `data_store_identifier` → `WKWebsiteDataStore(forIdentifier:)`
  - Windows: 같은 호출을 `crates/tauri-runtime-wry` 패치가 WebView2 프로필 이름(UUID hex)으로 바꾼다. 런타임 하나를 공유한다 (업스트림 tauri-apps/tauri#16054 와 같은 방식). 자세한 건 `crates/tauri-runtime-wry/PANEFUL_PATCH.md`
- 내비게이션 허용 목록(`src-tauri/src/nav.rs`) 밖 링크는 시스템 브라우저로
- 설정은 `settings.json` (앱 설정 폴더: Windows `%APPDATA%\dev.machrie.paneful`, macOS `~/Library/Application Support/dev.machrie.paneful`)
- 단축키 설정(파싱, 겹침 검사, 패널용 바인딩)은 `src-tauri/src/keys.rs`. `settings.json`의 `keys`(앱 단축키), `x_keys`(옮긴 X 단축키), `hotkey`(전역), `save_dir`(저장 폴더)에 들어간다
- 패널 배율은 `src-tauri/src/zoom.rs`(단계), `settings.json`의 계정별 `zoom`(100%면 안 적음). 웹뷰 `set_zoom`(WebView2 `ZoomFactor`, WKWebView `pageZoom`)으로 적용하고, 페이지를 새로 읽을 때마다 다시 적용한다
- 알림 숫자는 `src-tauri/src/unread.rs`(값 검사, 합계, 트레이 아이콘 빨간 점). 패널 스크립트가 X 화면의 배지를 읽어 `panel_unread`로 보낸다. 저장하지 않는다
- 트레이·전역 단축키는 `src-tauri/src/tray.rs` (`tray-icon` 피처, `tauri-plugin-global-shortcut`)
- 패널 주입 스크립트
  - `src-tauri/src/panel.js`: 포커스 보고, 앱 단축키(`Ctrl+휠` 배율 포함), 복사한 링크 열기(`paste` 단축키, 기본 `Ctrl+O`. `Ctrl+V`로 두면 입력창 밖에서만), 알림·쪽지 배지 읽기만. `panel` capability로 `panel_focused`, `panel_unread`(숫자만, Rust가 형식 검사), `shortcut`, `panel_keys`·`panel_xkeys`(단축키 바인딩 읽기), `paste_link`(클립보드는 Rust가 읽고 링크 검사·정리는 `nav.rs`의 `x_link`, 이동은 안 함), `open_link`(사용자가 확인한 주소를 `x_link`로 다시 검사하고 이동), `send_link`(다른 패널에서 열기: 주소를 `x_link`로 검사하고 고른 패널을 이동), `panel_stuck`(X 로딩 화면에 멈춘 패널을 홈으로 보냈다는 로그만)만 허용. 이동은 문서를 새로 불러오지 않고 `paneful-go` 이벤트로 페이지 안에서 바꾼다(`pushState` + `popstate`, X가 자기 링크로 이동할 때와 같음). X가 아직 안 떴거나 x.com이 아니면 그때만 새로 불러온다. 페이지가 15초 지나도 X 화면(`booted` 선택자)을 못 그리면 `x.com/home`으로 보내고(1분에 한 번), 그 상태에서 `Ctrl/⌘+R`을 누르면 바로 홈으로 간다. 창을 트레이로 숨길 때 영상 정지도 여기서 한다
  - `src-tauri/src/links.js`: x.com 계열 밖 링크의 왼쪽 클릭을 x.com보다 먼저 받아 실제 주소로 `window.open`. `on_new_window`가 기본 브라우저로 보낸다(우클릭 > 새 창과 같은 경로)
  - `src-tauri/src/rules.js` + `rules.rs` + `rules/x.v1.json`: X 화면 규칙(선택자). Rust가 받고 검증·캐시하고, `rules.js`가 `panel.js`·`media.js`·`capture.js`에 넘긴다. 패널은 `panel_rules`로 읽기만 한다
  - `src-tauri/src/media.js` + `media.rs`: 원본 저장 버튼. 패널엔 IPC 없이 `window.open` 트리거를 `on_new_window`가 가로채고, Rust가 twimg 주소를 검증한 뒤 쿠키 없이 받는다
  - `src-tauri/src/capture.js` + `capture.rs` + `snap.rs` + `ui/capture.js`: 게시물 캡쳐. 패널 스크립트가 범위를 스크롤하며 X의 고정 바를 숨기고, 자기 섀도 루트에 가림막(익명N)을 깔아 패널을 찍는다(원본 한 번, 가린 것 한 번). 스크린샷은 `snap.rs`가 OS 기능으로 찍는다(Windows `CapturePreview`, macOS `takeSnapshot`). Rust는 게시물 사각형만 잘라 두었다가 이어 붙이고, 미리보기·저장(`png`)·복사(`tauri-plugin-clipboard-manager`)를 맡는다. 찍기 전후로 위치를 재서 움직였으면 그 장면은 버린다

M0 스파이크 검증 체크표는 [m0-checklist.md](m0-checklist.md).

## CI와 릴리스

`.github/workflows/build.yml`이 PR마다(그리고 Actions 탭에서 수동 실행 시) Windows·macOS 설치 파일을 빌드해 Actions 아티팩트로 올린다 (받으려면 GitHub 로그인 필요, 기한 있음). main 푸시엔 빌드하지 않는다. 같은 브랜치에 새로 푸시하면 이전 실행은 취소된다.

릴리스는 둘 중 하나로 낸다. 어느 쪽이든 먼저 `src-tauri/tauri.conf.json`의 `version`을 올려 두고, 태그는 그 버전과 맞춘다(안 맞으면 릴리스 단계가 실패한다). 빌드가 깨지면 릴리스는 만들어지지 않는다.

- **브라우저만으로**: 저장소 Actions 탭 → 왼쪽 `build` → 오른쪽 **Run workflow** → Branch `main`, `release`에 `v0.0.1` 입력 → **Run workflow**. 빌드가 끝나면 그 커밋에 태그와 Release를 만든다
- **git으로**: `v`로 시작하는 태그를 push

  ```sh
  git tag v0.0.1
  git push origin v0.0.1
  ```

서명은 무료 경로만 쓴다. macOS는 ad-hoc 서명(`signingIdentity: "-"`), Windows는 미서명.

## X 화면 규칙 고치기

[`rules/x.v1.json`](../rules/x.v1.json)에서 값을 고치고 `revision`을 1 올려 main에 넣는다. 앱은 지금 쓰는 것보다 `revision`이 큰 파일만 받는다. 앱이 못 읽는 형식으로 바꿔야 하면 `format`을 올린 `x.v2.json`을 새로 만들고, 옛 앱을 위해 `x.v1.json`은 남겨 둔다. 이름 목록은 `src-tauri/src/rules.rs`의 `SELECTORS`, `TEXTS`.
