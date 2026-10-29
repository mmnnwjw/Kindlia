#!/bin/sh
# 基于 KinNovel 启动器：单实例、暂存二进制、暂停/恢复系统帧缓冲使用者。
# Kindlia 的路径均独立；两个阅读器可安装共存，同时只允许一个接管显示。
SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
APP_DIR=$(cd "$SCRIPT_DIR/.." && pwd)
export KINDLIA_HOME="${KINDLIA_HOME:-$APP_DIR}"
LOG_DIR="$KINDLIA_HOME/logs"
PAUSE_LIST=/tmp/kindlia_paused_pids
LOCK_DIR=/tmp/kindlia.lock
BIN_RUN=/tmp/kindlia-bin
GRAYSCALE_MARKER=/tmp/kindlia_grayscale
mkdir -p "$LOG_DIR" || exit 1
LOG_FILE="$LOG_DIR/kindlia.log"
log() { echo "[launcher] $*" >> "$LOG_FILE"; }
if [ -f "$LOG_FILE" ] && [ "$(wc -c < "$LOG_FILE")" -gt 1048576 ]; then
    tail -c 524288 "$LOG_FILE" > "$LOG_FILE.tmp" && mv "$LOG_FILE.tmp" "$LOG_FILE"
fi
# 不暂停正在运行的另一阅读器；请先正常退出它，避免互相接管系统进程。
if [ -f /tmp/kinnovel.lock/pid ]; then
    OTHER_PID=$(cat /tmp/kinnovel.lock/pid 2>/dev/null)
    if [ -n "$OTHER_PID" ] && kill -0 "$OTHER_PID" 2>/dev/null; then
        log "KinNovel is running; exit it before opening Kindlia"
        exit 1
    fi
fi
if ! mkdir "$LOCK_DIR" 2>/dev/null; then
    OLD_PID=$(cat "$LOCK_DIR/pid" 2>/dev/null)
    if [ -n "$OLD_PID" ] && kill -0 "$OLD_PID" 2>/dev/null && grep -q kindlia "/proc/$OLD_PID/cmdline" 2>/dev/null; then
        log "already running as $OLD_PID"; exit 1
    fi
    rm -f "$LOCK_DIR/pid"
    rmdir "$LOCK_DIR" 2>/dev/null || exit 1
    mkdir "$LOCK_DIR" || exit 1
fi
echo $$ > "$LOCK_DIR/pid"
resume_fb_users() {
    [ -f "$PAUSE_LIST" ] || return 0
    while read -r pid; do
        [ -n "$pid" ] && kill -CONT "$pid" 2>/dev/null
    done < "$PAUSE_LIST"
    rm -f "$PAUSE_LIST"
}
cleanup() {
    resume_fb_users
    [ "$SLEEP_COVER" = 1 ] && lipc-set-prop com.lab126.blanket load screensaver >/dev/null 2>&1
    lipc-set-prop com.lab126.appmgrd start app://com.lab126.booklet.home >/dev/null 2>&1 || true
    rm -f "$BIN_RUN" "$LOCK_DIR/pid"
    rmdir "$LOCK_DIR" 2>/dev/null || true
}
restore_display() {
    [ -f "$GRAYSCALE_MARKER" ] && [ -f "$BIN_RUN" ] && "$BIN_RUN" --restore-display >> "$LOG_FILE" 2>&1
    return 0
}
on_signal() {
    restore_display; cleanup; trap - EXIT INT TERM; exit "$1"
}
trap cleanup EXIT
trap 'on_signal 130' INT
trap 'on_signal 143' TERM
resume_fb_users
SLEEP_COVER=0
if grep -q '^sleep_cover *= *true' "$KINDLIA_HOME/config.toml" 2>/dev/null; then
    case "$(lipc-get-prop com.lab126.blanket load 2>/dev/null)" in
        *ad_screensaver*) log "keeping special offers screensaver" ;;
        *screensaver*) lipc-set-prop com.lab126.blanket unload screensaver 2>/dev/null && SLEEP_COVER=1 ;;
    esac
fi
export KINDLIA_SLEEP_COVER=$SLEEP_COVER
cp "$SCRIPT_DIR/kindlia" "$BIN_RUN" && chmod +x "$BIN_RUN" || { log "cannot stage binary"; exit 1; }
# 恢复上一次崩溃留下的显示状态。
restore_display
[ -n "$KN_ORIENTATION" ] || KN_ORIENTATION=$(lipc-get-prop com.lab126.winmgr accelerometer 2>/dev/null)
export KN_ORIENTATION
: > "$PAUSE_LIST"
for pid in $(ls -l /proc/[0-9]*/fd/ 2>/dev/null | awk '
    /^\/proc\/[0-9]+\/fd/ { split($0,p,"/"); pid=p[3]; next }
    $NF == "/dev/fb0" { print pid }' | sort -u); do
    [ "$pid" -ne $$ ] || continue
    if kill -STOP "$pid" 2>/dev/null; then echo "$pid" >> "$PAUSE_LIST"; fi
done
usleep 200000 2>/dev/null || sleep 1
log "starting Kindlia"
"$BIN_RUN" "$@" >> "$LOG_FILE" 2>&1
RET=$?
restore_display
log "exited with $RET"
exit "$RET"
