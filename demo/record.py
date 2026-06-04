#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = [
#   "pillow>=10,<12",
#   "pyte>=0.8,<0.9",
# ]
# ///

from __future__ import annotations

import fcntl
import json
import os
import pty
import select
import shutil
import signal
import struct
import subprocess
import sys
import termios
import time
from pathlib import Path

import pyte
from PIL import Image, ImageDraw, ImageFont, ImageStat

ROOT = Path("/work")
OUT = ROOT / "demo" / "out"
FRAMES = OUT / "frames"
CHECKS = OUT / "frame-checks"
VIDEO = OUT / "tmuxtui-demo.mp4"
VERIFY = OUT / "verification.txt"
COLS = 120
ROWS = 34
FPS = 12
FONT = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"
SYMBOL_CODES = (0x263C, 0x26B2)


class Terminal:
    def __init__(self, command: list[str], env: dict[str, str]) -> None:
        self.screen = pyte.Screen(COLS, ROWS)
        self.stream = pyte.Stream(self.screen)
        self.master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, 0, 0))
        attrs = termios.tcgetattr(slave)
        attrs[0] &= ~(termios.IXON | termios.IXOFF | getattr(termios, "IXANY", 0))
        termios.tcsetattr(slave, termios.TCSANOW, attrs)
        self.proc = subprocess.Popen(
            command,
            stdin=slave,
            stdout=slave,
            stderr=slave,
            env=env,
            cwd=ROOT,
            close_fds=True,
        )
        os.close(slave)
        flags = fcntl.fcntl(self.master, fcntl.F_GETFL)
        fcntl.fcntl(self.master, fcntl.F_SETFL, flags | os.O_NONBLOCK)

    def drain(self, seconds: float = 0.35) -> None:
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            ready, _, _ = select.select([self.master], [], [], max(0.0, end - time.monotonic()))
            if not ready:
                continue
            try:
                data = os.read(self.master, 65536)
            except (BlockingIOError, OSError):
                continue
            if not data:
                return
            self.stream.feed(data.decode("utf-8", "replace"))

    def send(self, value: bytes | str, pause: float = 0.22) -> None:
        os.write(self.master, value.encode() if isinstance(value, str) else value)
        self.drain(pause)

    def text(self, value: str) -> None:
        for ch in value:
            self.send(ch, 0.04)

    def backspace(self, count: int = 30) -> None:
        for _ in range(count):
            self.send(b"\x7f", 0.01)

    def command(self, value: str) -> None:
        self.send(":")
        self.text(value)
        self.send(b"\r")

    def close(self) -> None:
        if self.proc.poll() is None:
            self.proc.send_signal(signal.SIGTERM)
            try:
                self.proc.wait(timeout=2)
            except subprocess.TimeoutExpired:
                self.proc.kill()
        os.close(self.master)


class Renderer:
    def __init__(self) -> None:
        self.font = ImageFont.truetype(FONT, 16)
        self.bold = ImageFont.truetype(FONT, 16)
        self.symbol_fonts: dict[str, ImageFont.FreeTypeFont] = {}
        box = self.font.getbbox("M")
        self.cw = box[2] - box[0]
        self.ch = 21
        self.pad = 16
        self.caption_h = 42
        self.width = self.cw * COLS + self.pad * 2
        self.height = self.caption_h + self.ch * ROWS + self.pad * 2
        self.index = 0

    def hold(self, term: Terminal, caption: str, seconds: float = 1.0) -> None:
        term.drain(0.25)
        for _ in range(max(1, round(seconds * FPS))):
            self.render(term.screen, caption)

    def render(self, screen: pyte.Screen, caption: str) -> None:
        image = Image.new("RGB", (self.width, self.height), "#121212")
        draw = ImageDraw.Draw(image)
        draw.rectangle((0, 0, self.width, self.caption_h), fill="#202020")
        draw.text((self.pad, 12), caption, font=self.bold, fill="#eeeeee")
        ox = self.pad
        oy = self.caption_h + self.pad
        draw.rectangle((ox, oy, ox + self.cw * COLS, oy + self.ch * ROWS), fill="#1c1c1c")
        for y in range(ROWS):
            line = screen.buffer[y]
            for x in range(COLS):
                cell = line[x]
                fg = ansi_color(cell.fg, "#d7d7d7")
                bg = ansi_color(cell.bg, "#1c1c1c")
                if cell.reverse:
                    fg, bg = bg, fg
                x0 = ox + x * self.cw
                y0 = oy + y * self.ch
                if bg != "#1c1c1c":
                    draw.rectangle((x0, y0, x0 + self.cw, y0 + self.ch), fill=bg)
                if cell.data and cell.data != " ":
                    self.draw_cell(draw, x0, y0, cell.data, fg)
        cx = ox + screen.cursor.x * self.cw
        cy = oy + screen.cursor.y * self.ch + self.ch - 3
        draw.rectangle((cx, cy, cx + self.cw, cy + 1), fill="#eeeeee")
        image.save(FRAMES / f"{self.index:05}.png")
        self.index += 1

    def draw_cell(self, draw: ImageDraw.ImageDraw, x: int, y: int, text: str, fill: str) -> None:
        font = self.font_for(text)
        if font is self.font:
            draw.text((x, y + 2), text, font=font, fill=fill)
            return

        left, top, right, bottom = font.getbbox(text)
        dx = max(0, (self.cw - (right - left)) // 2)
        dy = max(0, (self.ch - (bottom - top)) // 2)
        draw.text((x + dx - left, y + dy - top), text, font=font, fill=fill)

    def font_for(self, text: str) -> ImageFont.FreeTypeFont:
        if len(text) != 1 or ord(text) < 128:
            return self.font
        if text not in self.symbol_fonts:
            self.symbol_fonts[text] = ImageFont.truetype(font_path_for(text), 16)
        return self.symbol_fonts[text]


def font_path_for(char: str) -> str:
    result = subprocess.run(
        ["fc-match", "-f", "%{file}", f":charset={ord(char):x}"],
        text=True,
        capture_output=True,
        check=True,
    )
    path = result.stdout.strip()
    if not path or not Path(path).exists():
        raise FileNotFoundError(f"no font found for U+{ord(char):04X}")
    return path


def symbol_font_report() -> list[str]:
    return [f"font_u+{code:04X}={font_path_for(chr(code))}" for code in SYMBOL_CODES]


def ansi_color(value: object, default: str) -> str:
    raw = str(value or "default")
    named = {
        "default": default,
        "black": "#000000",
        "red": "#d75f5f",
        "green": "#5faf5f",
        "yellow": "#d7af5f",
        "blue": "#5f87d7",
        "magenta": "#af5fd7",
        "cyan": "#5fd7d7",
        "white": "#eeeeee",
    }
    if raw in named:
        return named[raw]
    if len(raw) == 6 and all(ch in "0123456789abcdefABCDEF" for ch in raw):
        return f"#{raw.lower()}"
    return default


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


def render_cli_prelude(renderer: Renderer) -> None:
    screen = pyte.Screen(COLS, ROWS)
    stream = pyte.Stream(screen)
    stream.feed("$ tmuxtui --config\r\n$ tmuxtui\r\n")
    for _ in range(FPS):
        renderer.render(screen, "cli: --config refreshes managed tmux config")


def walk(term: Terminal, renderer: Renderer) -> None:
    h = renderer.hold
    h(term, "overview: minimal sidebar and live pane preview", 1.5)
    term.send("G")
    term.send("g")
    term.send("g")
    term.send("3j")
    term.send("k")
    h(term, "navigation: G, gg, count+j, k", 1.1)
    term.send(b"\t")
    h(term, "focus: tab moves into the preview pane", 0.8)
    term.send(b"\t")
    h(term, "focus: tab returns to the tree", 0.8)
    term.send("/")
    term.text("api")
    term.send(b"\r")
    term.send("n")
    term.send("N")
    h(term, "search: /api with next and previous matches", 1.1)
    term.send(" ")
    term.send("f")
    term.send("g")
    h(term, "grep: space f g opens the pane picker", 0.8)
    term.send("j")
    h(term, "grep: j starts narrowing pane captures", 0.6)
    term.send("o")
    h(term, "grep: jo narrows the pane list further", 0.6)
    term.send("b")
    h(term, "grep: job leaves three matching panes", 0.8)
    term.send(b"\x1b[B")
    h(term, "grep: down previews the second job match", 0.7)
    term.send(b"\x1b[B")
    h(term, "grep: down previews the third job match", 0.8)
    term.send(b"\r")
    h(term, "grep: enter attaches to the selected pane", 1.0)
    term.send(b"\x11")
    h(term, "detach: ctrl-q returns from the grep target to tmuxtui", 1.0)
    term.send("f")
    term.text("opss")
    term.backspace(1)
    h(term, "filter: backspace corrects the query", 0.8)
    term.send(b"\r")
    h(term, "filter: visible rows narrowed to ops", 1.0)
    term.send("f")
    term.send(b"\x1b")
    h(term, "escape: clear filter and return to normal mode", 0.8)
    term.send("g")
    term.send("g")
    term.send("O")
    term.text("design")
    term.send(b"\r")
    h(term, "create: O adds a peer session", 1.0)
    term.send("o")
    term.text("notes")
    term.send(b"\r")
    h(term, "create: o adds a child window", 1.0)
    term.send("o")
    h(term, "create: o on a window adds a pane", 1.0)
    term.send("S")
    h(term, "split: S creates a left/right split", 0.9)
    term.send("s")
    h(term, "split: s creates a top/bottom split", 0.9)
    term.send("z")
    h(term, "zoom: z toggles pane zoom", 0.8)
    term.send("z")
    h(term, "zoom: z restores the split layout", 0.8)
    term.send("f")
    term.text("design")
    term.send(b"\r")
    term.send("g")
    term.send("g")
    term.send("r")
    term.backspace()
    term.text("demo")
    term.send(b"\r")
    h(term, "rename: session renamed to demo", 0.9)
    term.send("f")
    term.backspace()
    term.text("demo")
    term.send(b"\r")
    term.send("g")
    term.send("g")
    term.send("j")
    term.send("r")
    term.backspace()
    term.text("notes")
    term.send(b"\r")
    h(term, "rename: window renamed to notes", 0.9)
    term.send("j")
    term.send("r")
    term.backspace()
    term.text("build")
    term.send(b"\r")
    h(term, "rename: pane title renamed to build", 0.9)
    term.command("pin")
    h(term, "pin: selected pane is marked for future attaches", 0.9)
    term.command("unpin")
    h(term, "unpin: clear the pinned pane marker", 0.9)
    term.send("a")
    h(term, "archive: a writes the selected pane capture", 0.9)
    term.send("A")
    h(term, "archive: A writes the whole window capture", 0.9)
    term.send("c")
    h(term, "caffeinate: Docker/Linux shows the macOS-only guard", 1.0)
    term.send("x")
    h(term, "cut: x cuts the selected pane", 0.9)
    term.send("f")
    term.send(b"\x1b")
    term.send("f")
    term.text("ops")
    term.send(b"\r")
    term.send("g")
    term.send("g")
    term.send("p")
    h(term, "paste: p moves the cut pane into ops as a window", 1.0)
    term.send("x")
    h(term, "cut: x cuts the pasted window", 0.9)
    term.send("f")
    term.send(b"\x1b")
    term.send("g")
    term.send("g")
    term.send("P")
    h(term, "paste: P moves the cut window into a new session", 1.0)
    term.send("O")
    term.text("trash")
    term.send(b"\r")
    term.send("o")
    term.text("tmp")
    term.send(b"\r")
    term.send("o")
    h(term, "create: temporary pane for removal actions", 0.9)
    term.send("d")
    h(term, "kill: d asks to remove the selected pane", 0.8)
    term.send("y")
    h(term, "kill: y confirms pane removal", 0.8)
    term.send("D")
    h(term, "kill: D asks to remove the full window", 0.8)
    term.send("y")
    h(term, "kill: y confirms full-window removal", 0.8)
    term.command("hidestatus")
    h(term, "command mode: hide tmux status for attached sessions", 0.8)
    term.command("showstatus")
    h(term, "command mode: restore tmux status", 0.8)
    term.send(b"\x12")
    h(term, "refresh: ctrl-r reloads tmux state", 0.8)
    term.send("f")
    term.text("api")
    term.send(b"\r")
    term.send("g")
    term.send("g")
    term.send("j")
    term.send(b"\r")
    h(term, "attach: enter opens the selected tmux target", 1.0)
    term.send(b"\x11")
    h(term, "detach: ctrl-q returns from tmux to tmuxtui", 1.2)
    term.send("f")
    term.text("api")
    term.send(b"\r")
    term.send("g")
    term.send("g")
    term.send("j")
    term.send("R")
    h(term, "remote tmux: R starts or attaches tmuxtui inside the pane", 1.0)
    term.send(b"\x11")
    h(term, "detach: ctrl-q returns after remote tmux attach", 1.2)
    term.send("q")
    term.drain(0.5)
    h(term, "quit: q exits the application", 0.8)


def encode_video() -> None:
    subprocess.run(["ffmpeg", "-y", "-framerate", str(FPS), "-i", str(FRAMES / "%05d.png"), "-vf", "format=yuv420p", "-movflags", "+faststart", str(VIDEO)], check=True, text=True, capture_output=True)


def verify_video() -> None:
    shutil.rmtree(CHECKS, ignore_errors=True)
    CHECKS.mkdir(parents=True, exist_ok=True)
    ffprobe = subprocess.run(["ffprobe", "-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height,nb_frames,duration", "-of", "json", str(VIDEO)], check=True, text=True, capture_output=True)
    subprocess.run(["ffmpeg", "-y", "-i", str(VIDEO), "-vf", "fps=1", str(CHECKS / "sample-%03d.png")], check=True, text=True, capture_output=True)
    scan = subprocess.run(["ffmpeg", "-i", str(VIDEO), "-vf", "blackdetect=d=0.5:pix_th=0.02,freezedetect=n=-60dB:d=2", "-an", "-f", "null", "-"], check=True, text=True, capture_output=True)
    scan_text = scan.stderr.strip()
    if "black_start" in scan_text:
        raise RuntimeError("ffmpeg black-frame verification failed")
    samples = sorted(CHECKS.glob("sample-*.png"))
    stats = [image_stats(path) for path in samples]
    bad = [item for item in stats if item["contrast"] < 20 or item["colors"] < 12]
    diffs = [frame_diff(left, right) for left, right in zip(samples, samples[1:])]
    moving = sum(diff > 0.35 for diff in diffs)
    if not samples or bad or moving < max(3, len(diffs) // 4):
        raise RuntimeError("ffmpeg frame verification failed")
    VERIFY.write_text("\n".join([f"video={VIDEO}", f"ffprobe={json.dumps(json.loads(ffprobe.stdout), sort_keys=True)}", *symbol_font_report(), f"samples={len(samples)}", f"min_contrast={min(item['contrast'] for item in stats):.2f}", f"min_colors={min(item['colors'] for item in stats)}", f"moving_pairs={moving}/{len(diffs)}", "ffmpeg_scan_stderr=", scan_text, ""]))


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
    shutil.rmtree(FRAMES, ignore_errors=True)
    FRAMES.mkdir(parents=True)
    env = setup_env()
    binary = build_binary(env)
    seed_tmux(env)
    renderer = Renderer()
    render_cli_prelude(renderer)
    term = Terminal([str(binary)], env)
    try:
        walk(term, renderer)
    finally:
        term.close()
        tmux(env, "kill-server", check=False)
    encode_video()
    verify_video()
    print(VIDEO)
    print(VERIFY)


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(f"demo recording failed: {error}", file=sys.stderr)
        raise
