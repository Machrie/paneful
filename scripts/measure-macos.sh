#!/bin/sh
# M0 memory check on macOS.
# WebKit content/network processes are XPC services, not children of the app,
# so quit Safari and other WebKit apps first or their numbers mix in.
# Usage: sh scripts/measure-macos.sh
sum() { ps -axo rss=,comm= | grep -E "$1" | grep -v grep | awk '{s+=$1; n++} END {printf "%d processes, %d MB RSS\n", n, s/1024}'; }
echo "== Paneful (app + com.apple.WebKit.*)"; sum 'Paneful|com\.apple\.WebKit\.'
echo "== Google Chrome";                     sum 'Google Chrome'
echo "== Microsoft Edge";                    sum 'Microsoft Edge'
