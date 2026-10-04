# Startup file of the demo shell; session.sh fills in @WORK@ and @SCENARIO@.
# It runs as root inside private namespaces, so these mounts are invisible
# outside the recording: DEMO_WORK becomes /home/alice and /home/bob, and
# /tmp is a fresh tmpfs.
set -e
mount --bind '@WORK@' /mnt
mount -t tmpfs tmpfs /home
mount -t tmpfs tmpfs /tmp
mkdir -p /home/alice /home/bob
mount --bind /mnt/alice /home/alice
mount --bind /mnt/bob /home/bob
hostname laptop
ip link set lo up
set +e

export HOME=/home/alice USER=alice LOGNAME=alice LANG=C.UTF-8 EDITOR=vi
export PATH=/home/alice/bin:/usr/local/bin:/usr/bin:/bin
export ARTIFACTIZE_STATE_HOME=$HOME/.local/state/artifactize

# Every shell, including tmux panes, gets the same short prompt.
cat >/home/alice/.pane-rc <<'EOF'
export PATH=/home/alice/bin:/usr/local/bin:/usr/bin:/bin LANG=C.UTF-8
PS1='\[\e[38;2;62;224;143m\]${USER}@$(hostname)\[\e[0m\] \[\e[38;2;139;150;168m\]\w\[\e[0m\] $ '
EOF
. /home/alice/.pane-rc

case '@SCENARIO@' in
reuse) cd ~/shop ;;
change)
    cd ~/shop
    artifactize verify --all >/dev/null 2>&1
    ;;
human) cd ~/brand ;;
team)
    # One review store on this machine's loopback, a token for each laptop.
    cd ~
    srv=/home/alice/.review-store
    alice_token=$(artifactize --state-dir "$srv" server token add alice-laptop --scopes read,publish | head -n 1)
    bob_token=$(artifactize --state-dir "$srv" server token add bob-laptop --scopes read,publish | head -n 1)
    (artifactize --state-dir "$srv" server run >"$srv.log" 2>&1 &)
    for _ in $(seq 50); do
        printf '%s\n' "$alice_token" | artifactize remote login http://127.0.0.1:8417/ >>~/.setup.log 2>&1 && break
        sleep 0.1
    done
    printf '%s\n' "$bob_token" | HOME=/home/bob USER=bob ARTIFACTIZE_STATE_HOME=/home/bob/.local/state/artifactize \
        artifactize remote login http://127.0.0.1:8417/ >>~/.setup.log 2>&1
    unset alice_token bob_token
    cd ~/shop
    ;;
esac
clear
