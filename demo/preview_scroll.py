#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = [
#   "pillow>=10,<12",
# ]
# ///

from __future__ import annotations

import json
import os
import shutil
import signal
import subprocess
import sys
import time
from pathlib import Path

from PIL import Image, ImageStat

ROOT = Path("/work")
OUT = ROOT / "demo" / "out"
CHECKS = OUT / "preview-scroll-frames"
VIDEO = OUT / "preview-scroll-demo.mp4"
VERIFY = OUT / "preview-scroll-verification.txt"
DISPLAY = ":98"
WIDTH = 1232
HEIGHT = 788
FPS = 12
TARGET_LINE = "300"


def run(args: list[str], env: dict[str, str], check: bool = True) -> subprocess.CompletedProcess[str]:
    return subprocess.run(args, cwd=ROOT, env=env, text=True, capture_output=True, check=check)


def setup_env() -> dict[str, str]:
    base = Path("/tmp/tmuxtui-demo")
    shutil.rmtree(base, ignore_errors=True)
    for path in [base / "home", base / "config", base / "tmux"]:
        path.mkdir(parents=True, exist_ok=True)
    os.chmod(base / "tmux", 0o700)
    env = os.environ.copy()
    env.pop("TMUX", None)
    env.update(
        {
            "HOME": str(base / "home"),
            "XDG_CONFIG_HOME": str(base / "config"),
            "TMUX_TMPDIR": str(base / "tmux"),
            "TERM": "xterm-256color",
            "SHELL": "/bin/bash",
            "PS1": "$ ",
        }
    )
    return env


def tmux(env: dict[str, str], *args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
    return run(["tmux", *args], env, check)


def seed_tmux(env: dict[str, str]) -> None:
    tmux(env, "kill-server", check=False)
    tmux(env, "new-session", "-d", "-s", "scroll", "-n", "counter", "-c", "/work", "bash -i")
    tmux(env, "set-option", "-g", "pane-border-style", "fg=colour245")
    tmux(env, "set-option", "-g", "pane-active-border-style", "fg=colour34")


def build_binary(env: dict[str, str]) -> Path:
    run(["cargo", "build", "--locked", "--bin", "tmuxtui"], env)
    config_dir = Path(env["XDG_CONFIG_HOME"]) / "tmuxtui"
    config_dir.mkdir(parents=True, exist_ok=True)
    (config_dir / "settings.conf").write_text(
        "show_hints=false\nshow_status=true\nsidebar_percent=12\nsidebar_auto=true\n"
    )
    binary = Path(env["CARGO_TARGET_DIR"]) / "debug" / "tmuxtui"
    run([str(binary), "--config"], env)
    return binary


class ScreenRecording:
    def __init__(self, binary: Path, env: dict[str, str]) -> None:
        self.env = env.copy()
        self.env["DISPLAY"] = DISPLAY
        self.binary = binary
        self.procs: list[subprocess.Popen[str]] = []
        self.keycast_path = Path(env["HOME"]).parent / "keys.txt"
        self.keycast_path.write_text("")
        self.keycast_until = 0.0
        self.window = ""

    def start(self) -> None:
        self._spawn(["Xvfb", DISPLAY, "-screen", "0", f"{WIDTH}x{HEIGHT}x24", "-nolisten", "tcp"])
        self._wait_for_display()
        self._spawn(
            [
                "xterm",
                "-geometry",
                "120x34+0+0",
                "-fa",
                "DejaVu Sans Mono",
                "-fs",
                "16",
                "-bg",
                "#1c1c1c",
                "-fg",
                "#d7d7d7",
                "-xrm",
                "XTerm.vt100.allowTitleOps: false",
                "-e",
                str(self.binary),
            ]
        )
        self.window = self._find_window()
        self._spawn_keycast()
        self.focus()
        self.procs.append(
            subprocess.Popen(
                [
                    "ffmpeg",
                    "-y",
                    "-video_size",
                    f"{WIDTH}x{HEIGHT}",
                    "-framerate",
                    str(FPS),
                    "-f",
                    "x11grab",
                    "-i",
                    f"{DISPLAY}.0",
                    "-vf",
                    "format=yuv420p",
                    "-movflags",
                    "+faststart",
                    str(VIDEO),
                ],
                cwd=ROOT,
                env=self.env,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
        )
        time.sleep(1.0)

    def _spawn(self, args: list[str]) -> subprocess.Popen[str]:
        proc = subprocess.Popen(args, cwd=ROOT, env=self.env, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.procs.append(proc)
        return proc

    def _spawn_keycast(self) -> None:
        script = f"""
printf '\033[?25l'
while true; do
  now=$(date +%s%3N)
  exp=$(cut -d' ' -f1 {self.keycast_path} 2>/dev/null)
  text=$(cut -d' ' -f2- {self.keycast_path} 2>/dev/null)
  [ -n "$exp" ] && [ "$now" -le "$exp" ] || text=
  printf '\033[2J\033[Hkeys: %s' "$text"
  sleep 0.08
done
"""
        self._spawn(
            [
                "xterm",
                "-geometry",
                "34x2+845+724",
                "-fa",
                "DejaVu Sans Mono",
                "-fs",
                "15",
                "-bg",
                "#202020",
                "-fg",
                "#eeeeee",
                "-bd",
                "#202020",
                "-xrm",
                "XTerm.vt100.scrollBar: false",
                "-e",
                "bash",
                "-lc",
                script,
            ]
        )
        time.sleep(0.2)

    def _wait_for_display(self) -> None:
        for _ in range(80):
            if run(["xdotool", "getdisplaygeometry"], self.env, check=False).returncode == 0:
                return
            time.sleep(0.1)
        raise RuntimeError("X display not ready")

    def _find_window(self) -> str:
        for _ in range(80):
            result = run(["xdotool", "search", "--onlyvisible", "--class", "XTerm"], self.env, check=False)
            ids = result.stdout.split()
            if ids:
                return ids[-1]
            time.sleep(0.1)
        raise RuntimeError("xterm window not found")

    def focus(self) -> None:
        run(["xdotool", "windowactivate", "--sync", self.window], self.env, check=False)
        run(["xdotool", "windowfocus", "--sync", self.window], self.env, check=False)

    def show_keys(self, value: str, append: bool = False) -> None:
        now = time.monotonic()
        shown = ""
        if append and now < self.keycast_until:
            parts = self.keycast_path.read_text().split(" ", 1)
            shown = parts[1] if len(parts) == 2 else ""
        sep = "" if not shown or len(value) == 1 else " "
        shown = f"{shown}{sep}{value}"
        self.keycast_until = now + 1.0
        expires = round(time.time() * 1000) + 1000
        self.keycast_path.write_text(f"{expires} {shown}")

    def key(self, *keys: str, pause: float = 0.35) -> None:
        self.show_keys(" ".join(display_key(key) for key in keys), append=True)
        self.focus()
        for key in keys:
            run(["xdotool", "key", key], self.env)
        time.sleep(pause)

    def text(self, value: str, pause: float = 0.08) -> None:
        self.show_keys(value, append=True)
        self.focus()
        run(["xdotool", "type", "--delay", str(round(pause * 1000)), "--", value], self.env)
        time.sleep(0.25)

    def hold(self, seconds: float = 1.0) -> None:
        time.sleep(seconds * 1.25)

    def backspace(self, count: int = 30) -> None:
        self.key(*(["BackSpace"] * count), pause=0.05)

    def command(self, value: str) -> None:
        self.key("colon", pause=0.08)
        self.text(value)
        self.key("Return")

    def close(self) -> None:
        for proc in reversed(self.procs):
            if proc.poll() is None:
                proc.send_signal(signal.SIGINT if proc.args and proc.args[0] == "ffmpeg" else signal.SIGTERM)
                try:
                    proc.wait(timeout=4)
                except subprocess.TimeoutExpired:
                    proc.kill()


def display_key(key: str) -> str:
    labels = {
        "BackSpace": "⌫",
        "Down": "Down",
        "Escape": "Esc",
        "Return": "Enter",
        "colon": ":",
        "slash": "/",
        "space": "Space",
    }
    if key.startswith("ctrl+"):
        return "Ctrl-" + key[5:]
    return labels.get(key, key)


def walk(term: ScreenRecording) -> None:
    h = term.hold
    h(1.5)
    term.key("Return")
    h(0.8)
    term.text('for i in $(seq -w 0 300); do echo "$i"; sleep 0.05; done; sleep 30')
    term.key("Return")
    h(0.5)
    # Detach directly so the preview opens while the counter is still actively scrolling.
    tmux(term.env, "detach-client", check=False)
    h(0.5)
    for _ in range(80):
        visible = tmux(term.env, "capture-pane", "-J", "-p", "-t", "scroll:counter", check=False).stdout
        lines = [line for line in visible.splitlines() if line.strip()]
        if lines and lines[-1] == TARGET_LINE:
            break
        time.sleep(0.25)
    h(2.0)


def verify_video(env: dict[str, str]) -> None:
    shutil.rmtree(CHECKS, ignore_errors=True)
    CHECKS.mkdir(parents=True, exist_ok=True)
    ffprobe = subprocess.run(["ffprobe", "-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height,nb_frames,duration", "-of", "json", str(VIDEO)], check=True, text=True, capture_output=True)
    subprocess.run(["ffmpeg", "-y", "-i", str(VIDEO), "-vf", "fps=1", str(CHECKS / "sample-%03d.png")], check=True, text=True, capture_output=True)
    scan = subprocess.run(["ffmpeg", "-i", str(VIDEO), "-vf", "freezedetect=n=-60dB:d=2", "-an", "-f", "null", "-"], check=True, text=True, capture_output=True)
    scan_text = scan.stderr.strip()
    samples = sorted(CHECKS.glob("sample-*.png"))
    stats = [image_stats(path) for path in samples]
    bad = [item for item in stats if item["contrast"] < 20 or item["colors"] < 8]
    diffs = [frame_diff(left, right) for left, right in zip(samples, samples[1:])]
    moving = sum(diff > 0.35 for diff in diffs)
    if not samples or bad or moving < 3:
        raise RuntimeError("ffmpeg frame verification failed")
    final_frame = OUT / "preview-scroll-final.png"
    subprocess.run(["ffmpeg", "-y", "-sseof", "-0.2", "-i", str(VIDEO), "-frames:v", "1", str(final_frame)], check=True, text=True, capture_output=True)
    visible = tmux(env, "capture-pane", "-J", "-p", "-t", "scroll:counter", check=False).stdout
    lines = [line for line in visible.splitlines() if line.strip()]
    if not lines or "000" in lines or lines[-1] != TARGET_LINE:
        raise RuntimeError(f"preview capture did not show scrolled bottom: {lines[-8:]}")
    VERIFY.write_text("\n".join([f"video={VIDEO}", f"final_frame={final_frame}", f"ffprobe={json.dumps(json.loads(ffprobe.stdout), sort_keys=True)}", f"samples={len(samples)}", f"min_contrast={min(item['contrast'] for item in stats):.2f}", f"min_colors={min(item['colors'] for item in stats)}", f"moving_pairs={moving}/{len(diffs)}", f"visible_tail={lines[-8:]}", f"contains_000={'000' in lines}", f"tail_contains_target={lines[-1] == TARGET_LINE}", "ffmpeg_scan_stderr=", scan_text, ""]))


def image_stats(path: Path) -> dict[str, float]:
    image = Image.open(path).convert("L")
    stat = ImageStat.Stat(image)
    extrema = image.getextrema()
    colors = len(image.resize((160, 90)).convert("P", palette=Image.ADAPTIVE, colors=64).getcolors())
    return {"contrast": float(extrema[1] - extrema[0]), "mean": stat.mean[0], "colors": colors}


def frame_diff(left: Path, right: Path) -> float:
    a = Image.open(left).convert("L").resize((160, 90))
    b = Image.open(right).convert("L").resize((160, 90))
    return sum(abs(x - y) for x, y in zip(a.tobytes(), b.tobytes())) / (160 * 90)


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    env = setup_env()
    binary = build_binary(env)
    seed_tmux(env)
    recording = ScreenRecording(binary, env)
    try:
        recording.start()
        walk(recording)
    finally:
        recording.close()
    verify_video(env)
    tmux(env, "kill-server", check=False)
    print(VIDEO)
    print(VERIFY)
    print(OUT / "preview-scroll-final.png")


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(f"demo recording failed: {error}", file=sys.stderr)
        raise
