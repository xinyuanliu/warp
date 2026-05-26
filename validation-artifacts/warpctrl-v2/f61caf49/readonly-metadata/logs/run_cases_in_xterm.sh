#!/usr/bin/env bash
set +e
export WARPCTRL="/workspace/warpctrl-validation/readonly-metadata/target/debug/warpctrl"
export WARP_LOCAL_CONTROL_DISCOVERY_DIR="/tmp/warpctrl-validation/readonly-metadata/final-home/discovery"
export HOME="/tmp/warpctrl-validation/readonly-metadata/final-home"
export XDG_RUNTIME_DIR="/tmp/warpctrl-validation/readonly-metadata/final-home/runtime"
export WARP_DATA_PROFILE="readonly-metadata-final-f61caf49"
export DISPLAY=":94"
while [ ! -f "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/logs/start_cases" ]; do sleep 0.1; done
echo "WarpCtrl readonly metadata validation terminal"
echo "Target Warp window: $WARP_WIN"
printf "\033c"
echo "### 001 window_list"
echo "Q: What is the best way to show the impact of this CLI command?"
echo "A: Use a staggered screenshot with this terminal command/output and the visible Warp window/tab/pane/session target."
echo "Proof setup: metadata reads enabled; one visible Warp window should be listed"
echo "$ $WARPCTRL --output-format json window list"
xdotool windowfocus "$WARP_WIN" >/dev/null 2>&1 || true
sleep 0.25
bash -lc "$WARPCTRL --output-format json window list" 2>&1 | tee "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/logs/001__outside-staggered__metadata__window_list__stdout_stderr.txt"
code=${PIPESTATUS[0]}
echo "$code" > "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/logs/001__outside-staggered__metadata__window_list__exit_code.txt"
echo "[exit_code=$code]"
sleep 0.6
scrot "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/screenshots/001__outside-staggered__metadata__window_list__terminal_ui.png"
sleep 0.2
printf "\033c"
echo "### 002 window_inspect_active"
echo "Q: What is the best way to show the impact of this CLI command?"
echo "A: Use a staggered screenshot with this terminal command/output and the visible Warp window/tab/pane/session target."
echo "Proof setup: active Warp window is focused before invocation, so inspected id should match visible Warp window"
echo "$ $WARPCTRL --output-format json window inspect --window active"
xdotool windowfocus "$WARP_WIN" >/dev/null 2>&1 || true
sleep 0.25
bash -lc "$WARPCTRL --output-format json window inspect --window active" 2>&1 | tee "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/logs/002__outside-staggered__metadata__window_inspect_active__stdout_stderr.txt"
code=${PIPESTATUS[0]}
echo "$code" > "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/logs/002__outside-staggered__metadata__window_inspect_active__exit_code.txt"
echo "[exit_code=$code]"
sleep 0.6
scrot "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/screenshots/002__outside-staggered__metadata__window_inspect_active__terminal_ui.png"
sleep 0.2
printf "\033c"
echo "### 003 tab_list"
echo "Q: What is the best way to show the impact of this CLI command?"
echo "A: Use a staggered screenshot with this terminal command/output and the visible Warp window/tab/pane/session target."
echo "Proof setup: visible tab strip should align with returned tabs"
echo "$ $WARPCTRL --output-format json tab list"
xdotool windowfocus "$WARP_WIN" >/dev/null 2>&1 || true
sleep 0.25
bash -lc "$WARPCTRL --output-format json tab list" 2>&1 | tee "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/logs/003__outside-staggered__metadata__tab_list__stdout_stderr.txt"
code=${PIPESTATUS[0]}
echo "$code" > "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/logs/003__outside-staggered__metadata__tab_list__exit_code.txt"
echo "[exit_code=$code]"
sleep 0.6
scrot "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/screenshots/003__outside-staggered__metadata__tab_list__terminal_ui.png"
sleep 0.2
printf "\033c"
echo "### 004 tab_inspect_active"
echo "Q: What is the best way to show the impact of this CLI command?"
echo "A: Use a staggered screenshot with this terminal command/output and the visible Warp window/tab/pane/session target."
echo "Proof setup: active visible bash tab should be returned"
echo "$ $WARPCTRL --output-format json tab inspect --tab active"
xdotool windowfocus "$WARP_WIN" >/dev/null 2>&1 || true
sleep 0.25
bash -lc "$WARPCTRL --output-format json tab inspect --tab active" 2>&1 | tee "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/logs/004__outside-staggered__metadata__tab_inspect_active__stdout_stderr.txt"
code=${PIPESTATUS[0]}
echo "$code" > "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/logs/004__outside-staggered__metadata__tab_inspect_active__exit_code.txt"
echo "[exit_code=$code]"
sleep 0.6
scrot "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/screenshots/004__outside-staggered__metadata__tab_inspect_active__terminal_ui.png"
sleep 0.2
printf "\033c"
echo "### 005 tab_inspect_index_0"
echo "Q: What is the best way to show the impact of this CLI command?"
echo "A: Use a staggered screenshot with this terminal command/output and the visible Warp window/tab/pane/session target."
echo "Proof setup: tab index 0 should resolve to the visible first tab"
echo "$ $WARPCTRL --output-format json tab inspect --tab-index 0"
xdotool windowfocus "$WARP_WIN" >/dev/null 2>&1 || true
sleep 0.25
bash -lc "$WARPCTRL --output-format json tab inspect --tab-index 0" 2>&1 | tee "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/logs/005__outside-staggered__metadata__tab_inspect_index_0__stdout_stderr.txt"
code=${PIPESTATUS[0]}
echo "$code" > "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/logs/005__outside-staggered__metadata__tab_inspect_index_0__exit_code.txt"
echo "[exit_code=$code]"
sleep 0.6
scrot "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/screenshots/005__outside-staggered__metadata__tab_inspect_index_0__terminal_ui.png"
sleep 0.2
printf "\033c"
echo "### 006 pane_list"
echo "Q: What is the best way to show the impact of this CLI command?"
echo "A: Use a staggered screenshot with this terminal command/output and the visible Warp window/tab/pane/session target."
echo "Proof setup: single visible terminal pane should be listed"
echo "$ $WARPCTRL --output-format json pane list"
xdotool windowfocus "$WARP_WIN" >/dev/null 2>&1 || true
sleep 0.25
bash -lc "$WARPCTRL --output-format json pane list" 2>&1 | tee "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/logs/006__outside-staggered__metadata__pane_list__stdout_stderr.txt"
code=${PIPESTATUS[0]}
echo "$code" > "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/logs/006__outside-staggered__metadata__pane_list__exit_code.txt"
echo "[exit_code=$code]"
sleep 0.6
scrot "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/screenshots/006__outside-staggered__metadata__pane_list__terminal_ui.png"
sleep 0.2
printf "\033c"
echo "### 007 pane_inspect_active"
echo "Q: What is the best way to show the impact of this CLI command?"
echo "A: Use a staggered screenshot with this terminal command/output and the visible Warp window/tab/pane/session target."
echo "Proof setup: active visible terminal pane should be returned"
echo "$ $WARPCTRL --output-format json pane inspect --pane active"
xdotool windowfocus "$WARP_WIN" >/dev/null 2>&1 || true
sleep 0.25
bash -lc "$WARPCTRL --output-format json pane inspect --pane active" 2>&1 | tee "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/logs/007__outside-staggered__metadata__pane_inspect_active__stdout_stderr.txt"
code=${PIPESTATUS[0]}
echo "$code" > "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/logs/007__outside-staggered__metadata__pane_inspect_active__exit_code.txt"
echo "[exit_code=$code]"
sleep 0.6
scrot "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/screenshots/007__outside-staggered__metadata__pane_inspect_active__terminal_ui.png"
sleep 0.2
printf "\033c"
echo "### 008 pane_inspect_index_0"
echo "Q: What is the best way to show the impact of this CLI command?"
echo "A: Use a staggered screenshot with this terminal command/output and the visible Warp window/tab/pane/session target."
echo "Proof setup: pane index 0 should resolve to visible terminal pane"
echo "$ $WARPCTRL --output-format json pane inspect --pane-index 0"
xdotool windowfocus "$WARP_WIN" >/dev/null 2>&1 || true
sleep 0.25
bash -lc "$WARPCTRL --output-format json pane inspect --pane-index 0" 2>&1 | tee "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/logs/008__outside-staggered__metadata__pane_inspect_index_0__stdout_stderr.txt"
code=${PIPESTATUS[0]}
echo "$code" > "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/logs/008__outside-staggered__metadata__pane_inspect_index_0__exit_code.txt"
echo "[exit_code=$code]"
sleep 0.6
scrot "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/screenshots/008__outside-staggered__metadata__pane_inspect_index_0__terminal_ui.png"
sleep 0.2
printf "\033c"
echo "### 009 session_list"
echo "Q: What is the best way to show the impact of this CLI command?"
echo "A: Use a staggered screenshot with this terminal command/output and the visible Warp window/tab/pane/session target."
echo "Proof setup: visible terminal session should be listed"
echo "$ $WARPCTRL --output-format json session list"
xdotool windowfocus "$WARP_WIN" >/dev/null 2>&1 || true
sleep 0.25
bash -lc "$WARPCTRL --output-format json session list" 2>&1 | tee "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/logs/009__outside-staggered__metadata__session_list__stdout_stderr.txt"
code=${PIPESTATUS[0]}
echo "$code" > "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/logs/009__outside-staggered__metadata__session_list__exit_code.txt"
echo "[exit_code=$code]"
sleep 0.6
scrot "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/screenshots/009__outside-staggered__metadata__session_list__terminal_ui.png"
sleep 0.2
printf "\033c"
echo "### 010 session_inspect_active"
echo "Q: What is the best way to show the impact of this CLI command?"
echo "A: Use a staggered screenshot with this terminal command/output and the visible Warp window/tab/pane/session target."
echo "Proof setup: active terminal session in the visible pane should be returned"
echo "$ $WARPCTRL --output-format json session inspect --session active"
xdotool windowfocus "$WARP_WIN" >/dev/null 2>&1 || true
sleep 0.25
bash -lc "$WARPCTRL --output-format json session inspect --session active" 2>&1 | tee "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/logs/010__outside-staggered__metadata__session_inspect_active__stdout_stderr.txt"
code=${PIPESTATUS[0]}
echo "$code" > "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/logs/010__outside-staggered__metadata__session_inspect_active__exit_code.txt"
echo "[exit_code=$code]"
sleep 0.6
scrot "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/screenshots/010__outside-staggered__metadata__session_inspect_active__terminal_ui.png"
sleep 0.2
printf "\033c"
echo "### 011 window_inspect_missing_window_id"
echo "Q: What is the best way to show the impact of this CLI command?"
echo "A: Use a staggered screenshot with this terminal command/output and the visible Warp window/tab/pane/session target."
echo "Proof setup: invalid opaque window id should return a structured missing/stale target error without changing UI"
echo "$ $WARPCTRL --output-format json window inspect --window missing-window-id"
xdotool windowfocus "$WARP_WIN" >/dev/null 2>&1 || true
sleep 0.25
bash -lc "$WARPCTRL --output-format json window inspect --window missing-window-id" 2>&1 | tee "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/logs/011__outside-staggered__metadata__window_inspect_missing_window_id__stdout_stderr.txt"
code=${PIPESTATUS[0]}
echo "$code" > "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/logs/011__outside-staggered__metadata__window_inspect_missing_window_id__exit_code.txt"
echo "[exit_code=$code]"
sleep 0.6
scrot "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/screenshots/011__outside-staggered__metadata__window_inspect_missing_window_id__terminal_ui.png"
sleep 0.2
printf "\033c"
echo "### 012 tab_inspect_index_999"
echo "Q: What is the best way to show the impact of this CLI command?"
echo "A: Use a staggered screenshot with this terminal command/output and the visible Warp window/tab/pane/session target."
echo "Proof setup: out-of-range visible tab index should return missing target or structured selector error"
echo "$ $WARPCTRL --output-format json tab inspect --tab-index 999"
xdotool windowfocus "$WARP_WIN" >/dev/null 2>&1 || true
sleep 0.25
bash -lc "$WARPCTRL --output-format json tab inspect --tab-index 999" 2>&1 | tee "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/logs/012__outside-staggered__metadata__tab_inspect_index_999__stdout_stderr.txt"
code=${PIPESTATUS[0]}
echo "$code" > "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/logs/012__outside-staggered__metadata__tab_inspect_index_999__exit_code.txt"
echo "[exit_code=$code]"
sleep 0.6
scrot "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/screenshots/012__outside-staggered__metadata__tab_inspect_index_999__terminal_ui.png"
sleep 0.2
printf "\033c"
echo "### 013 window_inspect_active_conflict_window_index_0"
echo "Q: What is the best way to show the impact of this CLI command?"
echo "A: Use a staggered screenshot with this terminal command/output and the visible Warp window/tab/pane/session target."
echo "Proof setup: conflicting selector flags should be rejected by CLI parsing"
echo "$ $WARPCTRL --output-format json window inspect --window active --window-index 0"
xdotool windowfocus "$WARP_WIN" >/dev/null 2>&1 || true
sleep 0.25
bash -lc "$WARPCTRL --output-format json window inspect --window active --window-index 0" 2>&1 | tee "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/logs/013__outside-staggered__metadata__window_inspect_active_conflict_window_index_0__stdout_stderr.txt"
code=${PIPESTATUS[0]}
echo "$code" > "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/logs/013__outside-staggered__metadata__window_inspect_active_conflict_window_index_0__exit_code.txt"
echo "[exit_code=$code]"
sleep 0.6
scrot "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/screenshots/013__outside-staggered__metadata__window_inspect_active_conflict_window_index_0__terminal_ui.png"
sleep 0.2
touch "/workspace/warpctrl-validation/readonly-metadata/validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata/logs/cases_done"
sleep 120
