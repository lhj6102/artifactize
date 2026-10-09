# Embedded Human review: verify waits by default; in the monitor, Enter drills from the Run to
# the eval and opens its Detail pane, c claims the request, Tab reaches the tools, the notes tool
# runs, g opens the GREEN schema form and Ctrl-S submits. Record with website/demo/record.sh
# (from the repository root). 14 px keeps the monitor above 100 columns, so the tree stays
# beside the focused pane.
Output website/demo/media/human.gif
Output website/demo/media/human.mp4
Source website/demo/settings.tape
Set FontSize 14
Set Height 860
Set LineHeight 1.1
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
Wait+Screen /waiting Human/
Sleep 2.5s
Enter
Wait+Screen /Enter: open/
Sleep 2.5s
Enter
Wait+Screen /Unclaimed\. c claims/
Sleep 2.5s
Type "c"
Wait+Screen /claimed by you/
Sleep 2s
Tab
Sleep 1.5s
Enter
Wait+Screen /Confirm Human tool/
Sleep 2s
Enter
Wait+Screen /16 px/
Sleep 2.5s
Type "g"
Wait+Screen /GREEN fields/
Sleep 1.5s
Tab
Sleep 500ms
Type "Matches the notes; reads well at 16 px."
Sleep 1.5s
Ctrl+S
Wait+Screen /Completed result/
Sleep 3.5s
Escape
Sleep 2s
Type "q"
Sleep 500ms
Ctrl+B
Type "z"
Sleep 3.5s
