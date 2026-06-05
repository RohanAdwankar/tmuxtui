# tmuxtui demo

`demo/run.sh` builds a Docker image, runs `tmuxtui` only inside that container, records a feature walkthrough, and writes:

- `demo/out/tmuxtui-demo.mp4`
- `demo/tmuxtui-demo.gif`
- `demo/out/frame-checks/`
- `demo/out/verification.txt`

The recorder builds an isolated tmux server, opens `tmuxtui` in xterm under Xvfb, drives the window with xdotool, records the X display with ffmpeg, then checks extracted frames for contrast and motion.
