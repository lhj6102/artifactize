# Reuse: status predicts, verify reviews everything once, the next verify reuses it all.
# Record with website/demo/record.sh (from the repository root).
Output website/demo/media/reuse.gif
Output website/demo/media/reuse.mp4
Source website/demo/settings.tape
Set Height 830
Set LoopOffset 92%

Hide
Type "exec website/demo/session.sh reuse" Enter
Sleep 3s
Show

Sleep 600ms
Type "artifactize status" Sleep 400ms Enter
Sleep 5s
Ctrl+L
Sleep 300ms
Type "artifactize verify --all" Sleep 400ms Enter
Sleep 3s
Type "artifactize verify --all" Sleep 400ms Enter
Sleep 6s
