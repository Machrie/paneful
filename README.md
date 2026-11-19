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
   - **카메라 버튼**: 게시물 캡쳐 (아래 [게시물 캡쳐](#게시물-캡쳐))
   - **눈 가림 버튼**: 창을 트레이로 숨기기 (아래 [트레이로 숨기기](#트레이로-숨기기))
   - **키보드 버튼**: 단축키 목록
   - **톱니 버튼**: 설정 (아래 [설정](#설정))

### 단축키

기본값이다. `Ctrl+1~9`를 뺀 나머지는 [설정](#설정)에서 바꾸거나 끌 수 있고, 단축키 목록(`Ctrl+/`)은 바꾼 키를 그대로 보여준다. macOS에선 `Ctrl` 대신 `⌘`를 쓴다. 패널 전환의 `Ctrl+Tab`만 맥에서도 `Ctrl`이다.

| 키 | 동작 |
|---|---|
| `Ctrl+1` ~ `Ctrl+9` | N번째 패널로 이동 (고정) |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | 다음 / 이전 패널 |
| `Ctrl+Shift+H` | 상단 바 보이기 / 숨기기 |
| `Ctrl+Shift+S` | 게시물 캡쳐 (아래 [게시물 캡쳐](#게시물-캡쳐)) |
| `Ctrl+/` | 단축키 목록 (`Esc`로 닫기) |
| `Ctrl+,` | 설정 (`Esc`로 닫기) |
| `Ctrl+Alt+X` (맥 `⌘⌥X`) | 창 숨기기 / 불러오기. 다른 앱이 앞에 있어도 동작 |

x.com 자체 단축키(`j`/`k` 이동, `l` 좋아요, `n` 새 게시물 등)는 활성 패널에 그대로 먹힌다. 바의 버튼을 눌러도 키보드 포커스는 활성 패널로 돌아간다. 전체 목록은 패널에서 `?`.

### 이미지·동영상 원본 저장

게시물의 이미지나 동영상에 마우스를 올리면 오른쪽 위에 **⤓ 원본** 버튼이 뜬다. 누르면 그 파일 하나를 원본 화질로 저장 폴더(기본 `다운로드/Paneful`, [설정](#설정)에서 변경)에 저장한다.

- 이미지는 원본 크기, 동영상·GIF는 가장 높은 화질의 mp4로 받는다
- 파일 이름은 `유저_게시물ID_순번`이다. 같은 이름이 있으면 `(2)`를 붙인다
- 동영상에서 "새로고침 후 다시"가 뜨면 그 패널을 새로 고친 뒤 다시 누른다
- 비공개 계정의 미디어는 저장이 안 될 수 있다
- 받은 파일의 저작권은 원작자에게 있다

### 설정

상단 바의 톱니 버튼, `Ctrl+,`, 또는 트레이 메뉴 **설정**으로 연다. 바꾸면 바로 적용되고 저장된다.

- **저장 폴더**: 이미지·동영상 다운로드와 캡쳐 PNG 저장 위치. 기본은 `다운로드/Paneful`. **변경**은 폴더 선택 창, **열기**는 탐색기(Finder)로 열기, **기본값**은 기본 폴더로 되돌리기
- **단축키**: 키 칸을 누르고 새 조합을 누르면 바뀐다(`Esc` 취소). 줄마다 **기본값**, **끄기**가 있다
  - `Ctrl`·`Alt`·`⌘` 중 하나는 같이 눌러야 한다(글 쓰다가 눌리는 것 방지). `F1`~`F24`는 단독도 된다
  - 다른 Paneful 단축키나 `Ctrl+1~9`와 겹치면 거부한다
  - 전역 단축키를 다른 앱이 쓰고 있으면 그 줄에 등록 실패가 뜨고 이전 키가 유지된다
  - 키 입력을 기다리는 동안엔 전역 단축키가 창을 숨기지 않는다
### 게시물 캡쳐

카카오톡 대화 캡쳐처럼 게시물 여러 개를 골라 PNG 한 장으로 만든다. 기본은 익명이다.

1. 패널에서 `Ctrl+Shift+S`(맥 `⌘⇧S`)를 누르거나 상단 바의 **카메라** 버튼을 누른다
2. 첫 게시물을 클릭한다(초록 테두리). 이 동안 게시물을 눌러도 열리지 않는다
3. 스크롤해서 마지막 게시물을 클릭한다. 위아래 순서는 상관없다. 같은 게시물을 한 번 더 누르면 그 하나만 찍는다
4. 앱이 첫 게시물부터 마지막 게시물까지 알아서 스크롤하며 모은다. 끝나면 원래 위치로 돌아오고 미리보기 창이 뜬다
5. 미리보기에서 **복사**(`Ctrl+C`), **PNG 저장**(`Ctrl+S`), **저장 + 복사**(`Enter`). 닫기는 `Esc`. 파일은 저장 폴더(기본 `다운로드/Paneful`)의 `paneful_날짜-시각.png`

`Esc`로 언제든 취소한다.

- **익명화**(기본 켜짐): 사람마다 `익명1`, `익명2`처럼 번호를 붙이고 프로필 사진을 번호 동그라미로 바꾼다. 같은 사람은 캡쳐 안에서 같은 번호다. 본문 속 `@멘션`, 인용 게시물 작성자, 본문에 그대로 적힌 범위 안 사람들의 표시 이름도 바꾼다
- **시간**: 게시 시각 표시를 끄고 켠다
- 그림은 X 화면을 찍은 게 아니라 모은 내용으로 앱이 다시 그린 카드다. 글, 이미지, 동영상 썸네일, 링크 카드, 인용 게시물이 들어간다. 투표, 답글·좋아요 수, "님이 재게시함" 같은 줄은 빠진다
- 한 번에 최대 150개. 긴 글이 "더 보기"로 잘린 경우, 페이지가 이미 받아 둔 전문이 있으면 전문을, 없으면 잘린 부분까지만 넣고 그렇게 표시한다
- 익명화가 못 지우는 것: 이미지 안의 이름이나 얼굴, 본문에 멘션 없이 적힌 범위 밖 사람 이름, 글 내용 자체(검색하면 원문이 나올 수 있다). 공유 전에 직접 확인한다
- 이미지는 "원본 저장"과 같은 방식으로 앱이 쿠키 없이 `pbs.twimg.com`에서 받는다. X에 따로 요청을 보내지 않는다

### 트레이로 숨기기

- 상단 바의 **눈 가림** 버튼이나 전역 단축키로 창을 숨긴다. 숨기면 작업 표시줄·Alt+Tab(Windows), Dock·Cmd+Tab(macOS)에서도 사라지고 트레이(macOS는 메뉴 막대) 아이콘만 남는다
- 다시 띄우기: 전역 단축키, 또는 트레이 아이콘 클릭(Windows) / 메뉴 막대 아이콘 → **Paneful 보이기/숨기기**(macOS)
- 전역 단축키 기본값은 Windows `Ctrl+Alt+X`, macOS `⌘⌥X`. 다른 앱이 앞에 있어도 먹는다. Paneful이 앞에 있으면 숨기고, 숨겨져 있거나 뒤에 있으면 앞으로 불러온다
- 단축키는 [설정](#설정)에서 바꾸거나 끈다. 다른 앱이 이미 잡고 있으면 등록이 실패하고 설정 화면에 이유가 뜨며, 원래 키가 그대로 남는다
- 트레이 메뉴의 **설정**으로 창을 띄우면서 설정을 연다. **종료**로 완전히 끈다. 창의 닫기(X) 버튼은 지금처럼 앱을 끈다

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
- 설정은 `settings.json` (앱 설정 폴더: Windows `%APPDATA%\dev.machrie.paneful`, macOS `~/Library/Application Support/dev.machrie.paneful`)
- 단축키 설정(파싱, 겹침 검사, 패널용 바인딩)은 `src-tauri/src/keys.rs`. `settings.json`의 `keys`(앱 단축키), `hotkey`(전역), `save_dir`(저장 폴더)에 들어간다
- 트레이·전역 단축키는 `src-tauri/src/tray.rs` (`tray-icon` 피처, `tauri-plugin-global-shortcut`)
- 패널 주입 스크립트
  - `src-tauri/src/panel.js`: 포커스 보고와 앱 단축키만. `panel` capability로 `panel_focused`, `shortcut`, `panel_keys`(단축키 바인딩 읽기)만 허용
  - `src-tauri/src/media.js` + `media.rs`: 원본 저장 버튼. 패널엔 IPC 없이 `window.open` 트리거를 `on_new_window`가 가로채고, Rust가 twimg 주소를 검증한 뒤 쿠키 없이 받는다
  - `src-tauri/src/capture.js` + `capture.rs` + `ui/capture.js`: 게시물 캡쳐. 패널 스크립트가 범위를 스크롤하며 DOM에서 게시물을 읽어 `capture_posts`(패널이 부를 수 있는 세 번째 커맨드)로 넘긴다. 호스트 UI가 캔버스에 카드로 그리고 익명화하며, 캔버스는 4000px 단위 조각 PNG로 Rust에 넘어가 한 장으로 이어 붙인 뒤 저장(`png`)·복사(`tauri-plugin-clipboard-manager`)된다

M0 스파이크 검증 체크표는 [docs/m0-checklist.md](docs/m0-checklist.md).

### CI와 릴리스

`.github/workflows/build.yml`이 PR마다(그리고 Actions 탭에서 수동 실행 시) Windows·macOS 설치 파일을 빌드해 Actions 아티팩트로 올린다 (받으려면 GitHub 로그인 필요, 기한 있음). main 푸시엔 빌드하지 않는다. 같은 브랜치에 새로 푸시하면 이전 실행은 취소된다.

릴리스는 둘 중 하나로 낸다. 어느 쪽이든 먼저 `src-tauri/tauri.conf.json`의 `version`을 올려 두고, 태그는 그 버전과 맞춘다(안 맞으면 릴리스 단계가 실패한다). 빌드가 깨지면 릴리스는 만들어지지 않는다.

- **브라우저만으로**: 저장소 Actions 탭 → 왼쪽 `build` → 오른쪽 **Run workflow** → Branch `main`, `release`에 `v0.0.1` 입력 → **Run workflow**. 빌드가 끝나면 그 커밋에 태그와 Release를 만든다
- **git으로**: `v`로 시작하는 태그를 push

  ```sh
  git tag v0.0.1
  git push origin v0.0.1
  ```

서명은 무료 경로만 쓴다. macOS는 ad-hoc 서명(`signingIdentity: "-"`), Windows는 미서명.

## 라이선스

[MIT](LICENSE). `crates/tauri-runtime-wry`는 Tauri 원본 라이선스(MIT 또는 Apache-2.0)를 따른다.
