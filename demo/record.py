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
CHECKS = OUT / "frame-checks"
VIDEO = OUT / "tmuxtui-demo.mp4"
VERIFY = OUT / "verification.txt"
DISPLAY = ":99"
WIDTH = 1232
HEIGHT = 788
FPS = 12


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
    tmux(env, "new-session", "-d", "-s", "api", "-n", "logs", "-c", "/work", "bash -lc 'printf \"api service\\nGET /health 200\\nGET /v1/jobs 200\\n\"; while sleep 4; do printf \"worker synced queue\\n\"; done'")
    tmux(env, "set-option", "-g", "pane-border-style", "fg=colour245")
    tmux(env, "set-option", "-g", "pane-active-border-style", "fg=colour245")
    tmux(env, "split-window", "-h", "-t", "api:logs", "-c", "/work", "bash -lc 'printf \"request stream\\nPOST /v1/jobs 202\\ncache hit user:42\\n\"; while sleep 5; do printf \"poll complete\\n\"; done'")
    tmux(env, "new-window", "-t", "api", "-n", "console", "-c", "/work", "bash -lc 'printf \"curl http://localhost:8080/health\\n{status: ok}\\n\"; exec bash -i'")
    tmux(env, "new-session", "-d", "-s", "docs", "-n", "readme", "-c", "/work", "bash -lc 'sed -n \"1,70p\" README.md; exec bash -i'")
    tmux(env, "new-session", "-d", "-s", "ops", "-n", "deploy", "-c", "/work", "bash -lc 'printf \"deploy checklist\\n[ok] build\\n[ok] tests\\n[pending] release\\n\"; exec bash -i'")
    tmux(env, "split-window", "-v", "-t", "ops:deploy", "-c", "/work", "bash -lc 'printf \"queue\\njob-1842 running\\njob-1843 waiting\\n\"; exec bash -i'")
    tmux(env, "select-window", "-t", "api:logs")


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
    term.key("g")
    term.key("g")
    term.key("O")
    term.text("design")
    term.key("Return")
    h(1.0)
    term.key("o")
    term.text("notes")
    term.key("Return")
    h(1.0)
    term.key("o")
    h(0.8)
    term.key("Return")
    h(1.0)
    term.key("ctrl+q")
    h(0.7)
    term.key("S")
    h(0.7)
    term.key("Return")
    h(1.0)
    term.key("ctrl+q")
    h(0.7)
    term.key("s")
    h(0.7)
    term.key("Return")
    h(1.0)
    for key in ("ctrl+l", "ctrl+j", "ctrl+h", "ctrl+k"):
        term.key(key)
        h(0.8)
    term.text("vi .")
    term.key("Return")
    h(1.2)
    term.key("ctrl+q")
    h(0.7)
    term.key("z")
    h(0.7)
    term.key("Return")
    h(1.0)
    term.key("ctrl+q")
    h(0.7)
    term.key("z")
    h(0.7)
    term.key("Return")
    h(1.0)
    term.key("ctrl+q")
    h(0.7)
    term.key("G")
    term.key("g")
    term.key("g")
    term.key("3", "j")
    term.key("k")
    h(1.1)
    term.key("slash")
    term.text("api")
    term.key("Return")
    term.key("n")
    term.key("N")
    h(1.1)
    term.key("space")
    term.key("f")
    term.key("g")
    h(0.8)
    term.key("j")
    h(0.6)
    term.key("o")
    h(0.6)
    term.key("b")
    h(0.8)
    term.key("Down")
    h(0.7)
    term.key("Down")
    h(0.8)
    term.key("Return")
    h(1.0)
    term.key("ctrl+q")
    h(1.0)
    term.key("f")
    term.text("ops")
    h(0.8)
    term.key("Return")
    h(1.0)
    term.key("f")
    term.key("Escape")
    h(0.8)
    term.key("g")
    term.key("g")
    term.key("r")
    term.key("ctrl+u")
    term.text("demo")
    term.key("Return")
    h(0.9)
    term.key("f")
    term.key("ctrl+u")
    term.text("demo")
    term.key("Return")
    term.key("g")
    term.key("g")
    term.key("j")
    term.key("r")
    term.key("ctrl+u")
    term.text("notes")
    term.key("Return")
    h(0.9)
    term.key("j")
    term.key("r")
    term.key("ctrl+u")
    term.text("build")
    term.key("Return")
    h(0.9)
    term.command("pin")
    h(1.0)
    term.key("j")
    h(0.8)
    term.key("Return")
    h(1.2)
    term.key("ctrl+q")
    h(0.9)
    term.key("j")
    h(0.8)
    term.key("Return")
    h(1.2)
    term.key("ctrl+q")
    h(0.9)
    for _ in range(6):
        term.key("j")
        h(0.18)
    h(0.8)
    term.key("Return")
    h(1.2)
    term.key("ctrl+q")
    h(0.9)
    term.command("unpin")
    h(0.9)
    term.key("a")
    h(0.9)
    term.key("A")
    h(0.9)
    term.key("c")
    h(1.0)
    term.key("x")
    h(0.9)
    term.key("f")
    term.key("Escape")
    term.key("f")
    term.text("ops")
    term.key("Return")
    term.key("g")
    term.key("g")
    term.key("p")
    h(1.0)
    term.key("x")
    h(0.9)
    term.key("f")
    term.key("Escape")
    term.key("g")
    term.key("g")
    term.key("P")
    h(1.0)
    term.key("O")
    term.text("trash")
    term.key("Return")
    term.key("o")
    term.text("tmp")
    term.key("Return")
    term.key("o")
    h(0.9)
    term.key("d")
    h(0.8)
    term.key("y")
    h(0.8)
    term.key("D")
    h(0.8)
    term.key("y")
    h(0.8)
    term.command("hidestatus")
    h(0.8)
    term.command("showstatus")
    h(0.8)
    term.key("ctrl+r")
    h(0.8)
    term.key("f")
    term.text("api")
    term.key("Return")
    term.key("g")
    term.key("g")
    term.key("j")
    term.key("Return")
    h(1.0)
    term.key("ctrl+q")
    h(1.2)
    term.key("f")
    term.text("api")
    term.key("Return")
    term.key("g")
    term.key("g")
    term.key("j")
    term.key("R")
    h(1.0)
    term.key("ctrl+q")
    h(1.2)
    term.key("q")
    h(0.8)


def verify_video() -> None:
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
    if not samples or bad or moving < max(3, len(diffs) // 4):
        raise RuntimeError("ffmpeg frame verification failed")
    VERIFY.write_text("\n".join([f"video={VIDEO}", f"ffprobe={json.dumps(json.loads(ffprobe.stdout), sort_keys=True)}", f"samples={len(samples)}", f"min_contrast={min(item['contrast'] for item in stats):.2f}", f"min_colors={min(item['colors'] for item in stats)}", f"moving_pairs={moving}/{len(diffs)}", "ffmpeg_scan_stderr=", scan_text, ""]))


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
        tmux(env, "kill-server", check=False)
    verify_video()
    print(VIDEO)
    print(VERIFY)


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(f"demo recording failed: {error}", file=sys.stderr)
        raise
