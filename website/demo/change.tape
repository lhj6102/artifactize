# Change one file: status predicts one execution and says why, verify re-reviews only that.
# Starts from a reviewed project. Record with website/demo/record.sh (from the repository root).
Output website/demo/media/change.gif
Output website/demo/media/change.mp4
Source website/demo/settings.tape
Set Height 830
Set LoopOffset 92%

Hide
Type "exec website/demo/session.sh change" Enter
Sleep 4s
Show

Sleep 600ms
Type `echo "Orders ship within two days." >> docs/guide.md` Sleep 400ms Enter
Sleep 800ms
Type "artifactize status" Sleep 400ms Enter
Sleep 6s
Ctrl+L
Sleep 300ms
Type "artifactize verify --all" Sleep 400ms Enter
Sleep 6s
