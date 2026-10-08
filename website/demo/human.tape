# Embedded Human review: verify waits by default; monitor opens the eval modal,
# explicitly claims the request, runs its tool and submits GREEN through the schema form.
# Record with website/demo/record.sh (from the repository root).
Output website/demo/media/human.gif
Output website/demo/media/human.mp4
Source website/demo/settings.tape
Set Height 960
Set LineHeight 1.05
Set LoopOffset 95%

Hide
Type "exec website/demo/session.sh human" Enter
Sleep 3s
Type "tmux -L artifactize-demo new-session -s demo \; select-pane -T 'terminal 1' \; split-window -v -l 26 \; select-pane -T 'terminal 2' \; select-pane -t 0" Enter
Sleep 1.5s
Show

Sleep 600ms
Type "artifactize verify --all" Sleep 400ms Enter
Wait+Screen /Started /
Sleep 2s
Ctrl+B
Down
Sleep 400ms
Ctrl+B
Type "z"
Sleep 400ms
Type "artifactize monitor" Sleep 400ms Enter
Wait+Screen /WAITING_HUMAN/
Sleep 2s
Enter
Sleep 500ms
Type "j"
Sleep 300ms
Type "j"
Sleep 1s
Type "o"
Wait+Screen /Unclaimed/
Sleep 2s
Type "c"
Wait+Screen /Choose GREEN or RED/
Sleep 1s
Tab
Sleep 400ms
Enter
Wait+Screen /Confirm Human tool/
Sleep 1.5s
Enter
Wait+Screen /16 px/
Sleep 3s
Tab
Sleep 400ms
Type "g"
Sleep 1.5s
Tab
Sleep 300ms
Type "Matches the notes; reads well at 16 px."
Sleep 2s
Ctrl+S
Wait+Screen /Completed result/
Sleep 4s
Escape
Sleep 3s
Type "q"
Sleep 500ms
Ctrl+B
Type "z"
Sleep 3s
