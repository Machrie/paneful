# Paneful

x.com만 여는 경량 데스크톱 앱. 한 창에 패널을 나란히 띄우고 패널마다 계정을 따로 로그인한다. Tauri 2 (Rust), Windows 10/11 + macOS 14+.

X Corp.과 무관한 비공식 앱이다. 앱은 어떤 데이터도 수집·전송하지 않는다. 비밀번호는 앱이 안 만지고, 로그인은 패널 안의 x.com 페이지에서 한다.

## 지금 상태: M0 스파이크

- 한 창 = 호스트 UI 웹뷰(상단 바, 패널 사이 틈) + 계정별 x.com 자식 웹뷰 N개 (`tauri` `unstable` 피처)
- 계정 = UUID = 데이터 저장소
  - macOS 14+: `data_store_identifier` → `WKWebsiteDataStore(forIdentifier:)`
  - Windows: 같은 호출을 `crates/tauri-runtime-wry` 패치가 WebView2 프로필 이름(UUID hex)으로 바꿈. 런타임 하나 공유 (업스트림 tauri-apps/tauri#16054 와 같은 방식)
- 패널 사이 6px 틈 드래그로 폭 조절, 비율은 `settings.json`에 저장
- 내비게이션 허용 목록 밖 링크(t.co 포함)는 시스템 브라우저로
- 콘텐츠 필터(CSS 숨김, 요청 차단)는 아직 없음 (M2, 기본 꺼짐 예정)

## 빌드

필요: Rust stable, Node 22 (Tauri CLI용). Windows는 WebView2 런타임(10/11 기본 포함), macOS는 Xcode CLT.

```sh
cargo run -p paneful                  # 개발 실행
npx @tauri-apps/cli@2 build           # 설치 파일 (Windows: NSIS .exe, macOS: .dmg)
```

서명은 아직 무료 경로만 쓴다. macOS는 ad-hoc 서명이라 첫 실행 때 우클릭 → 열기(또는 시스템 설정 → 개인정보 보호 및 보안에서 허용) 필요. Windows는 미서명이라 SmartScreen 경고가 뜬다.

CI(`.github/workflows/build.yml`)가 push/PR마다 Windows·macOS 설치 파일을 Actions 아티팩트로 올린다.

## M0 통과 조건 체크

| 항목 | 방법 | Win | Mac |
|---|---|---|---|
| 패널 2개에 서로 다른 계정 동시 로그인 | 그냥 로그인 | ☐ | ☐ |
| 앱 재시작 후 두 계정 로그인 유지 | 종료 후 재실행 | ☐ | ☐ |
| 틈 드래그 리사이즈 | 패널 사이 틈 끌기 | ☐ | ☐ |
| WebView2 런타임 1개 공유 | `scripts/measure-windows.ps1` 의 browser processes = 1 | ☐ | - |
| 프로필 이름 = 계정 UUID | 로그 `WebView2 profile name=... path=...` | ☐ | - |
| 계정 삭제 시 저장소 삭제 | 상단 "삭제" → 재추가 시 로그아웃 상태 | ☐ | ☐ |
| 메모리 vs Edge 프로필 2개 | `measure-windows.ps1` / `measure-macos.sh` | ☐ | ☐ |

로그 위치: Windows `%LOCALAPPDATA%\dev.machrie.paneful\logs`, macOS `~/Library/Logs/dev.machrie.paneful`.
