# Team review store: Alice's verify publishes to one artifactize server; Bob's status predicts
# remote reuse and his verify reuses all of Alice's verdicts. Both laptops are panes on one
# machine, each with its own checkout and state. Record with website/demo/record.sh.
Output website/demo/media/team.gif
Output website/demo/media/team.mp4
Source website/demo/settings.tape
Set Height 900
Set LineHeight 1.05
Set LoopOffset 95%

Hide
Type "exec website/demo/session.sh team" Enter
Sleep 5s
Type `tmux -L artifactize-demo new-session -s demo \; select-pane -T "alice's laptop" \; split-window -v -l 15 -e HOME=/home/bob -e USER=bob -e LOGNAME=bob -e ARTIFACTIZE_STATE_HOME=/home/bob/.local/state/artifactize -c /home/bob/shop \; select-pane -T "bob's laptop" \; select-pane -t 0` Enter
Sleep 1.5s
Show

Sleep 600ms
Type "artifactize remote status" Sleep 400ms Enter
Sleep 2.5s
Type "artifactize verify --all" Sleep 400ms Enter
Sleep 3.5s
Ctrl+B
Down
Sleep 400ms
Type "artifactize status | tail -n 1" Sleep 400ms Enter
Sleep 2.5s
Type "artifactize verify --all" Sleep 400ms Enter
Sleep 7s
