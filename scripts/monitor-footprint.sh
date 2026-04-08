#!/bin/bash
#
# 在 macOS 上采样 Usher 的内存 Footprint（与活动监视器「内存」列更接近）及 ps RSS。
# 用于对照「拖窗口 / 全屏」时的占用变化。
#
# 依赖: macOS 自带 footprint(1)（部分旧系统可能没有）
# 若使用 resize-demo: 需在「系统设置 → 隐私与安全性 → 辅助功能」中允许终端/Cursor。
#
# 用法:
#   ./scripts/monitor-footprint.sh              # 自动查找 target/debug/usher 或 usher，每 0.5s 打印一行
#   ./scripts/monitor-footprint.sh <PID>        # 指定进程
#   ./scripts/monitor-footprint.sh resize-demo  # 自动改窗口尺寸并各打一行采样（需 usher 已运行）
#
set -euo pipefail

INTERVAL="${INTERVAL:-0.5}"

require_footprint() {
  command -v footprint >/dev/null 2>&1 || {
    echo "错误: 未找到 footprint 命令（可能系统过旧）。" >&2
    exit 1
  }
}

resolve_pid() {
  local arg="${1:-}"
  if [[ -n "$arg" && "$arg" =~ ^[0-9]+$ ]]; then
    echo "$arg"
    return
  fi
  local p
  p=$(pgrep -f 'target/debug/usher' | head -1 || true)
  if [[ -n "$p" ]]; then
    echo "$p"
    return
  fi
  p=$(pgrep -x usher | head -1 || true)
  if [[ -n "$p" ]]; then
    echo "$p"
    return
  fi
  echo ""
}

footprint_mb() {
  local pid="$1"
  footprint "$pid" 2>/dev/null | head -3 | grep -oE 'Footprint: [0-9]+ MB' | grep -oE '[0-9]+' || echo "?"
}

rss_mb() {
  local pid="$1"
  local rss
  rss=$(ps -p "$pid" -o rss= 2>/dev/null | tr -d ' ')
  [[ -n "$rss" ]] || { echo "?"; return; }
  echo $((rss / 1024))
}

resize_window() {
  local w="$1" h="$2"
  osascript -e "tell application \"System Events\" to tell process \"usher\" to set frontmost to true" >/dev/null 2>&1
  osascript -e "tell application \"System Events\" to tell process \"usher\" to set size of window 1 to {$w, $h}" 2>/dev/null || true
}

set_fullscreen() {
  local on="$1"
  osascript -e "tell application \"System Events\" to tell process \"usher\" to set frontmost to true" >/dev/null 2>&1
  if [[ "$on" == "1" ]]; then
    osascript -e 'tell application "System Events" to tell process "usher" to set value of attribute "AXFullScreen" of window 1 to true' 2>/dev/null || true
  else
    osascript -e 'tell application "System Events" to tell process "usher" to set value of attribute "AXFullScreen" of window 1 to false' 2>/dev/null || true
  fi
}

run_resize_demo() {
  require_footprint
  local pid
  pid=$(resolve_pid "")
  if [[ -z "$pid" ]]; then
    echo "错误: 未找到 usher 进程。" >&2
    exit 1
  fi
  echo "PID=$pid"
  sample() {
    sleep 1.2
    local fp rss
    fp=$(footprint_mb "$pid")
    rss=$(rss_mb "$pid")
    printf '%s  %-18s  footprint=%s MB  rss=%s MB\n' "$(date +%H:%M:%S)" "$1" "$fp" "$rss"
  }
  sample "initial"
  resize_window 560 420 && sample "560x420"
  resize_window 1280 860 && sample "1280x860"
  resize_window 1680 1000 && sample "1680x1000"
  set_fullscreen 1 && sample "fullscreen"
  set_fullscreen 0 && sample "exit fullscreen"
  resize_window 560 420 && sample "560x420 again"
}

run_watch() {
  require_footprint
  local pid
  pid=$(resolve_pid "${1:-}")
  if [[ -z "$pid" ]]; then
    echo "错误: 未找到 usher。请运行 cargo run 或传入 PID。" >&2
    exit 1
  fi
  echo "PID=$pid — 每 ${INTERVAL}s 采样，Ctrl+C 结束"
  echo "time                 footprint_MB  rss_MB"
  while true; do
    if ! kill -0 "$pid" 2>/dev/null; then
      echo "进程已退出。"
      exit 0
    fi
    fp=$(footprint_mb "$pid")
    rss=$(rss_mb "$pid")
    printf '%s  %6s        %s\n' "$(date +%H:%M:%S)" "$fp" "$rss"
    sleep "$INTERVAL"
  done
}

case "${1:-}" in
  resize-demo)
    run_resize_demo
    ;;
  -h|--help|help)
    sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'
    ;;
  *)
    run_watch "${1:-}"
    ;;
esac
