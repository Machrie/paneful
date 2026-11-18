# M0 스파이크 통과 조건

개발 중 검증용 체크표. 사용자 안내는 [README](../README.md)를 보면 된다.

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
