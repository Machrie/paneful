# Paneful

x.com 전용 경량 데스크톱 앱. 한 창에 x.com 패널을 나란히 띄우고, 패널마다 다른 계정으로 로그인해 둔다. 브라우저 프로필을 여러 개 띄우거나 계정을 계속 전환할 필요가 없다.

- 패널 = 계정. 패널마다 쿠키·로그인 저장소가 완전히 분리된다
- 앱을 껐다 켜도 각 패널의 로그인이 유지된다
- 패널 사이 틈을 끌어 폭을 조절하고, 비율은 저장된다
- x.com 밖 링크(t.co 포함)는 기본 브라우저로 연다

> X Corp.과 무관한 비공식 앱이다. 앱은 어떤 데이터도 수집·전송하지 않는다. 비밀번호는 앱이 다루지 않고, 로그인은 패널 안의 x.com 페이지에서 직접 한다.

## 지원 환경

| OS | 버전 | 비고 |
|---|---|---|
| Windows | 10, 11 (64비트) | WebView2 런타임 필요. Windows 11과 최신 Windows 10에는 기본 포함 |
| macOS | 14 Sonoma 이상 | 계정별 저장소 분리에 macOS 14 API가 필요해서 13 이하는 지원 안 함. Apple Silicon·Intel 둘 다 |

## 설치

[Releases](https://github.com/Machrie/paneful/releases) 페이지에서 최신 버전의 파일을 받는다.

| OS | 파일 |
|---|---|
| Windows | `Paneful_<버전>_x64-setup.exe` |
| macOS | `Paneful_<버전>_universal.dmg` |

이 앱은 유료 코드 서명을 하지 않았다. 그래서 처음 실행할 때 OS가 경고를 띄운다. 아래 순서대로 한 번만 허용하면 이후엔 그냥 열린다.

### Windows

1. `Paneful_..._x64-setup.exe` 실행
2. "Windows의 PC 보호" (SmartScreen) 창이 뜨면 **추가 정보** → **실행** 클릭
3. 설치는 현재 사용자 계정에만 되고 관리자 권한은 필요 없다

**스마트 앱 컨트롤**(Windows 11, 설정 → 개인 정보 및 보안 → Windows 보안 → 앱 및 브라우저 컨트롤)이 켜져 있으면 서명 안 된 앱이라 실행이 막힌다. 이 경우엔 우회 버튼이 없다. 스마트 앱 컨트롤을 끌지는 각자 판단할 문제다.

### macOS

1. `.dmg`를 열고 `Paneful`을 `응용 프로그램` 폴더로 끌어 넣는다
2. `Paneful`을 실행한다. "Apple이 악성 코드가 없음을 확인할 수 없습니다" 같은 경고가 뜨면 **완료**(또는 취소)로 닫는다
3. 허용한다
   - **macOS 15 Sequoia 이상**: 시스템 설정 → 개인정보 보호 및 보안 → 아래쪽 보안 항목의 "Paneful … 차단되었습니다" 옆 **그래도 열기** 클릭 → 암호 입력 → 다시 뜨는 창에서 **열기**
   - **macOS 14 Sonoma**: Finder에서 `Paneful`을 우클릭(Control+클릭) → **열기** → **열기**. 안 되면 위 15 방식(시스템 설정)으로 한다
4. 위 방법이 다 안 되고 "손상되었기 때문에 열 수 없습니다"가 뜨면 터미널에서 다운로드 격리 표시를 지우고 다시 연다

   ```sh
   xattr -dr com.apple.quarantine /Applications/Paneful.app
   ```

## 사용법

1. 처음 열면 패널 두 개(계정 1, 계정 2)가 x.com 로그인 화면으로 뜬다. 패널마다 다른 계정으로 로그인한다
2. 더 필요하면 상단 바의 **＋ 계정**으로 패널을 추가한다
3. 패널 사이 틈을 끌어 폭을 조절한다
4. 패널 이름 옆 **새로고침**은 그 패널만 새로 고친다
5. **삭제**는 패널을 닫고 그 계정의 로그인 정보(쿠키·저장소)까지 지운다. 다시 추가하면 로그아웃된 새 패널로 시작한다 (확인 단계 한 번 있음)

x.com 자체 단축키는 클릭해서 포커스가 가 있는 패널에 적용된다.

## 제거

앱에서 계정을 먼저 **삭제**해 두면 로그인 정보까지 같이 지워진다. 그 다음:

- **Windows**: 설정 → 앱 → 설치된 앱에서 Paneful 제거. 설정과 로그 폴더가 남으면 `%APPDATA%\dev.machrie.paneful`, `%LOCALAPPDATA%\dev.machrie.paneful`을 지운다
- **macOS**: `응용 프로그램`의 `Paneful`을 휴지통으로. 설정은 `~/Library/Application Support/dev.machrie.paneful`, 로그는 `~/Library/Logs/dev.machrie.paneful`에 남는다

## 문제가 생기면

[Issues](https://github.com/Machrie/paneful/issues)에 OS 버전과 증상, 가능하면 로그를 같이 올려 달라.

로그 위치:

- Windows: `%LOCALAPPDATA%\dev.machrie.paneful\logs`
- macOS: `~/Library/Logs/dev.machrie.paneful`

---

## 개발

Tauri 2 (Rust). 프런트엔드는 빌드 단계 없는 정적 파일(`ui/`)이다.

### 필요한 것

- Rust stable
- Node 22: 설치 파일을 만드는 Tauri CLI(`npx @tauri-apps/cli`)를 돌릴 때만 쓴다. 앱 자체엔 Node가 들어가지 않는다
- Windows: WebView2 런타임, macOS: Xcode Command Line Tools

### 명령

```sh
cargo run -p paneful                  # 개발 실행
cargo test -p paneful --lib           # 단위 테스트
npx @tauri-apps/cli@2 build           # 설치 파일 (Windows: NSIS .exe, macOS: .dmg)
```

### 구조

- 한 창 = 호스트 UI 웹뷰(상단 바, 패널 사이 틈) + 계정별 x.com 자식 웹뷰 N개 (`tauri` `unstable` 피처)
- 계정 = UUID = 데이터 저장소
  - macOS 14+: `data_store_identifier` → `WKWebsiteDataStore(forIdentifier:)`
  - Windows: 같은 호출을 `crates/tauri-runtime-wry` 패치가 WebView2 프로필 이름(UUID hex)으로 바꾼다. 런타임 하나를 공유한다 (업스트림 tauri-apps/tauri#16054 와 같은 방식). 자세한 건 `crates/tauri-runtime-wry/PANEFUL_PATCH.md`
- 내비게이션 허용 목록(`src-tauri/src/nav.rs`) 밖 링크는 시스템 브라우저로
- 설정은 `settings.json` (앱 설정 폴더)

M0 스파이크 검증 체크표는 [docs/m0-checklist.md](docs/m0-checklist.md).

### CI와 릴리스

`.github/workflows/build.yml`이 push·PR마다 Windows·macOS 설치 파일을 빌드해 Actions 아티팩트로 올린다 (받으려면 GitHub 로그인 필요, 기한 있음).

`v`로 시작하는 태그를 push하면 같은 빌드 결과로 GitHub Release를 만들어 설치 파일을 붙인다. 태그는 `src-tauri/tauri.conf.json`의 `version`과 맞춘다.

```sh
git tag v0.0.1
git push origin v0.0.1
```

서명은 무료 경로만 쓴다. macOS는 ad-hoc 서명(`signingIdentity: "-"`), Windows는 미서명.
