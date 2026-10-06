# Human review in the terminal: verify waits for a sign-off; in the monitor, o hands
# off to review, which runs an output tool and submits through the form; verify finishes.
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
Type "tmux new-session -s demo \; select-pane -T 'terminal 1' \; split-window -v -l 26 \; select-pane -T 'terminal 2' \; select-pane -t 0" Enter
Sleep 1.5s
Show

Sleep 600ms
Type "artifactize verify --all" Sleep 400ms Enter
Sleep 2s
Ctrl+B
Down
Sleep 400ms
Type "artifactize monitor" Sleep 400ms Enter
Sleep 2.5s
Enter
Sleep 2s
Type "j"
Sleep 400ms
Type "j"
Sleep 1.5s
Type "o"
Sleep 2.5s
Enter
Sleep 2s
Type "y"
Sleep 3s
Type "s"
Sleep 1.2s
Type "g"
Sleep 1.5s
Tab
Sleep 300ms
Type "Matches the notes; reads well at 16 px."
Sleep 1s
Enter
Sleep 7s
