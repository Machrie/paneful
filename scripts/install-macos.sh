#!/bin/bash
# Install or update Paneful on macOS from the latest GitHub release.
#
#   curl -fsSL https://raw.githubusercontent.com/Machrie/paneful/main/scripts/install-macos.sh | bash
#
# Downloads the .dmg with curl (so the app gets no quarantine flag and
# opens without the Gatekeeper warning), quits a running Paneful, replaces
# /Applications/Paneful.app and starts it again. Settings and logins live
# in ~/Library and are not touched.
set -euo pipefail

REPO="Machrie/paneful"
APP="/Applications/Paneful.app"

say() { printf '%s\n' "$*"; }
die() { printf '실패: %s\n' "$*" >&2; exit 1; }

[ "$(uname -s)" = "Darwin" ] || die "맥 전용 스크립트"

# /releases/latest redirects to /releases/tag/vX.Y.Z. No API call, so no
# rate limit and no JSON to parse.
tag_url=$(curl -fsSLo /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest") \
  || die "GitHub 연결 안 됨"
tag=${tag_url##*/}
case "$tag" in
  v[0-9]*) ;;
  *) die "최신 버전을 못 찾음 ($tag_url)" ;;
esac
ver=${tag#v}

if [ -d "$APP" ]; then
  cur=$(defaults read "$APP/Contents/Info" CFBundleShortVersionString 2>/dev/null || true)
  if [ "$cur" = "$ver" ]; then
    say "이미 최신 버전 v$ver"
    exit 0
  fi
  say "v${cur:-?} -> v$ver"
else
  say "v$ver 설치"
fi

tmp=$(mktemp -d)
mnt="$tmp/mnt"
cleanup() {
  hdiutil detach -quiet "$mnt" >/dev/null 2>&1 || true
  rm -rf "$tmp"
}
trap cleanup EXIT

dmg="$tmp/Paneful.dmg"
say "받는 중…"
curl -fL --progress-bar -o "$dmg" \
  "https://github.com/$REPO/releases/download/$tag/Paneful_${ver}_universal.dmg" \
  || die "dmg 받기 실패"

mkdir "$mnt"
hdiutil attach -nobrowse -noautoopen -readonly -quiet -mountpoint "$mnt" "$dmg" </dev/null \
  || die "dmg 열기 실패"
[ -d "$mnt/Paneful.app" ] || die "dmg 안에 Paneful.app이 없음"

# Quit a running copy first. `quit app` would launch it when it is not
# running, so only ask when the process exists.
running() { pgrep -f "$APP/Contents/MacOS/" >/dev/null 2>&1; }
if running; then
  say "실행 중인 Paneful 끄는 중…"
  osascript -e 'quit app "Paneful"' >/dev/null 2>&1 || true
  for _ in $(seq 1 20); do
    running || break
    sleep 0.5
  done
  if running; then
    pkill -f "$APP/Contents/MacOS/" || true
    sleep 1
  fi
fi

[ -w /Applications ] || die "/Applications에 쓸 권한이 없음 (관리자 계정으로 실행)"
rm -rf "$APP"
ditto "$mnt/Paneful.app" "$APP" || die "응용 프로그램 폴더에 복사 실패"
# curl leaves no quarantine flag, but an older copy may have had one.
xattr -dr com.apple.quarantine "$APP" 2>/dev/null || true

say "v$ver 설치 완료, 실행합니다"
open "$APP"
