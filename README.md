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
| Windows (설치) | `Paneful_<버전>_x64-setup.exe` |
| Windows (포터블) | `Paneful_<버전>_x64-portable.zip` |
| macOS | `Paneful_<버전>_universal.dmg` |

이 앱은 유료 코드 서명을 하지 않았다. 그래서 처음 실행할 때 OS가 경고를 띄운다. 아래 순서대로 한 번만 허용하면 이후엔 그냥 열린다.

### Windows

1. `Paneful_..._x64-setup.exe` 실행
2. "Windows의 PC 보호" (SmartScreen) 창이 뜨면 **추가 정보** → **실행** 클릭
3. 설치는 현재 사용자 계정에만 되고 관리자 권한은 필요 없다

**포터블(zip)**: 설치 없이 압축을 풀고 `Paneful.exe`를 실행하면 된다. 처음 실행할 때 SmartScreen 경고는 설치판과 똑같이 뜬다.

- 계정 데이터(로그인, 설정, 로그)는 설치판과 같은 `%APPDATA%`·`%LOCALAPPDATA%`의 `dev.machrie.paneful`에 저장된다. exe 옆에 저장되지 않으니 USB에 넣고 다른 PC로 옮겨도 로그인은 따라가지 않는다. 설치판과 같은 PC에서 쓰면 계정을 공유한다
- WebView2 런타임이 필요하다. 설치판은 없으면 설치 중에 받아 오지만 포터블은 그렇지 않다. Windows 11과 업데이트된 Windows 10에는 이미 들어 있다. 실행하자마자 꺼지거나 아무 창도 안 뜨면 Microsoft의 [WebView2 런타임](https://developer.microsoft.com/microsoft-edge/webview2/)(Evergreen Bootstrapper)을 설치한다

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

### 기본

1. 처음 열면 패널 두 개(계정 1, 계정 2)가 x.com 로그인 화면으로 뜬다. 패널마다 다른 계정으로 로그인한다
2. 패널 사이 구분선을 끌어 폭을 조절한다. 비율은 저장된다
3. 상단 바는 기본으로 숨겨져 있고 맨 위에 얇은 줄만 남는다. 줄은 패널마다 한 칸씩이고 지금 활성 패널 칸이 계정 색으로 칠해진다. 이 줄을 클릭하거나 `Ctrl+Shift+H`를 누르면 바가 열린다. 바의 `^` 버튼이나 같은 단축키로 다시 숨긴다
4. 바에서 할 수 있는 것
   - **＋ 계정**: 패널 추가
   - 탭에 마우스를 올리면 나오는 **새로고침**: 그 패널만 새로 고침
   - 탭에 마우스를 올리면 나오는 **휴지통**: 패널을 닫고 그 계정의 로그인 정보(쿠키·저장소)까지 지운다. 확인을 한 번 더 묻는다. 다시 추가하면 로그아웃된 새 패널로 시작한다
   - **키보드 버튼**: 단축키 목록

### 단축키

macOS에선 `Ctrl` 대신 `⌘`를 쓴다. 패널 전환의 `Ctrl+Tab`만 맥에서도 `Ctrl`이다.

| 키 | 동작 |
|---|---|
| `Ctrl+1` ~ `Ctrl+9` | N번째 패널로 이동 |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | 다음 / 이전 패널 |
| `Ctrl+Shift+H` | 상단 바 보이기 / 숨기기 |
| `Ctrl+/` | 단축키 목록 (`Esc`로 닫기) |

x.com 자체 단축키(`j`/`k` 이동, `l` 좋아요, `n` 새 게시물 등)는 활성 패널에 그대로 먹힌다. 바의 버튼을 눌러도 키보드 포커스는 활성 패널로 돌아간다. 전체 목록은 패널에서 `?`.

### 이미지·동영상 원본 저장

게시물의 이미지나 동영상에 마우스를 올리면 오른쪽 위에 **⤓ 원본** 버튼이 뜬다. 누르면 그 파일 하나를 원본 화질로 `다운로드/Paneful` 폴더에 저장한다.

- 이미지는 원본 크기, 동영상·GIF는 가장 높은 화질의 mp4로 받는다
- 파일 이름은 `유저_게시물ID_순번`이다. 같은 이름이 있으면 `(2)`를 붙인다
- 동영상에서 "새로고침 후 다시"가 뜨면 그 패널을 새로 고친 뒤 다시 누른다
- 비공개 계정의 미디어는 저장이 안 될 수 있다
- 받은 파일의 저작권은 원작자에게 있다

## 제거

앱에서 계정을 먼저 **삭제**해 두면 로그인 정보까지 같이 지워진다. 그 다음:

- **Windows**: 설정 → 앱 → 설치된 앱에서 Paneful 제거(포터블은 폴더째 삭제). 설정과 로그 폴더가 남으면 `%APPDATA%\dev.machrie.paneful`, `%LOCALAPPDATA%\dev.machrie.paneful`을 지운다
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
- 패널 주입 스크립트
  - `src-tauri/src/panel.js`: 포커스 보고와 앱 단축키만. `panel` capability로 `panel_focused`, `shortcut` 두 커맨드만 허용
  - `src-tauri/src/media.js` + `media.rs`: 원본 저장 버튼. 패널엔 IPC 없이 `window.open` 트리거를 `on_new_window`가 가로채고, Rust가 twimg 주소를 검증한 뒤 쿠키 없이 받는다

M0 스파이크 검증 체크표는 [docs/m0-checklist.md](docs/m0-checklist.md).

### CI와 릴리스

`.github/workflows/build.yml`이 push·PR마다 Windows·macOS 설치 파일을 빌드해 Actions 아티팩트로 올린다 (받으려면 GitHub 로그인 필요, 기한 있음).

`v`로 시작하는 태그를 push하면 같은 빌드 결과로 GitHub Release를 만들어 설치 파일을 붙인다. 태그는 `src-tauri/tauri.conf.json`의 `version`과 맞춘다.

```sh
git tag v0.0.1
git push origin v0.0.1
```

서명은 무료 경로만 쓴다. macOS는 ad-hoc 서명(`signingIdentity: "-"`), Windows는 미서명.

## 라이선스

[MIT](LICENSE). `crates/tauri-runtime-wry`는 Tauri 원본 라이선스(MIT 또는 Apache-2.0)를 따른다.
