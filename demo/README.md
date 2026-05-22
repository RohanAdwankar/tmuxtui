# tmuxtui demo

`demo/run.sh` builds a Docker image, runs `tmuxtui` only inside that container, records a feature walkthrough, and writes:

- `demo/out/tmuxtui-demo.mp4`
- `demo/out/frame-checks/`
- `demo/out/verification.txt`

The recorder drives the real TUI through a pseudo terminal, renders decoded terminal frames, encodes them with ffmpeg, then uses ffmpeg-extracted frames for a nonblank and motion check.
