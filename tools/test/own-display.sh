# Sourced by the harnesses that draw (keyrings' prompts and windows): a
# display of their own, Xvfb on the first number from :90 up that has no
# lock and no socket, as smoke.py's. Never `-displayfd`: it tries :0
# first, and X's transport unlinks a socket's path before binding its own,
# so it removed the desktop's Xwayland socket (`/tmp/.X11-unix/X0`, which
# Hyprland's Xwayland serves as a file only, with no abstract one) and the
# session's new X applications failed until it was linked back.
#   own_display <Xvfb> <log>   sets `display` (":90") and `xvfb_pid`

own_display() {
    local xvfb=$1 log=$2 number
    for number in $(seq 90 199); do
        if [ -e "/tmp/.X$number-lock" ] || [ -e "/tmp/.X11-unix/X$number" ]; then
            continue
        fi
        "$xvfb" ":$number" -screen 0 1024x768x24 -nolisten tcp > "$log" 2>&1 &
        xvfb_pid=$!
        for _ in $(seq 200); do
            if [ -e "/tmp/.X11-unix/X$number" ]; then
                display=":$number"
                return 0
            fi
            kill -0 "$xvfb_pid" 2>/dev/null || break
            sleep 0.05
        done
        # Taken meanwhile by another, or never ready: the next.
        kill "$xvfb_pid" 2>/dev/null || true
        xvfb_pid=
    done
    echo "no display of its own could be started; see $log" >&2
    return 1
}
