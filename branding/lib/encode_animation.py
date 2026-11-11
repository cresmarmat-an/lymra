"""Encodes a directory of RGBA PNG frames into an animated GIF and WebP.

Usage: python3 encode_animation.py FRAME_DIR FPS OUTPUT_BASE

Writes OUTPUT_BASE.gif and OUTPUT_BASE.webp. Pillow is used instead of ffmpeg
because ffmpeg's GIF encoder stops storing only changed regions once the
palette has a transparent entry, which made every frame a full image.
"""

import sys
from pathlib import Path

from PIL import Image

TRANSPARENT_INDEX = 255
ALPHA_THRESHOLD = 128
# Clears each frame before the next, so moving parts leave no trail over transparent areas.
RESTORE_TO_BACKGROUND = 2
WEBP_QUALITY = 90


def load_frames(frame_dir: Path) -> list[Image.Image]:
    paths = sorted(frame_dir.glob("frame-*.png"))
    if not paths:
        raise SystemExit(f"No frame-*.png files in {frame_dir}")
    return [Image.open(path).convert("RGBA") for path in paths]


def shared_palette(frames: list[Image.Image]) -> Image.Image:
    """Builds one palette from every frame so colors never shift between frames."""
    width, height = frames[0].size
    strip = Image.new("RGB", (width, height * len(frames)))
    for index, frame in enumerate(frames):
        strip.paste(frame.convert("RGB"), (0, height * index))
    return strip.quantize(colors=TRANSPARENT_INDEX, method=Image.Quantize.MEDIANCUT)


def to_gif_frame(frame: Image.Image, palette: Image.Image) -> Image.Image:
    """Maps a frame onto the shared palette; GIF alpha is on or off, so it is thresholded."""
    indexed = frame.convert("RGB").quantize(palette=palette, dither=Image.Dither.NONE)
    clear = frame.getchannel("A").point(lambda alpha: 255 if alpha < ALPHA_THRESHOLD else 0)
    indexed.paste(TRANSPARENT_INDEX, mask=clear)
    return indexed


def write_gif(frames: list[Image.Image], frame_ms: int, output: Path) -> None:
    palette = shared_palette(frames)
    indexed = [to_gif_frame(frame, palette) for frame in frames]
    indexed[0].save(output, save_all=True, append_images=indexed[1:], duration=frame_ms, loop=0,
                    transparency=TRANSPARENT_INDEX, disposal=RESTORE_TO_BACKGROUND)


def write_webp(frames: list[Image.Image], frame_ms: int, output: Path) -> None:
    frames[0].save(output, save_all=True, append_images=frames[1:], duration=frame_ms, loop=0,
                   quality=WEBP_QUALITY, method=6)


def main() -> None:
    if len(sys.argv) != 4:
        raise SystemExit(__doc__)
    frame_dir, fps_text, output_base = Path(sys.argv[1]), sys.argv[2], sys.argv[3]
    fps = int(fps_text)
    if fps <= 0:
        raise SystemExit(f"FPS must be positive, got {fps}")
    frame_ms = round(1000 / fps)
    frames = load_frames(frame_dir)
    write_gif(frames, frame_ms, Path(f"{output_base}.gif"))
    write_webp(frames, frame_ms, Path(f"{output_base}.webp"))


if __name__ == "__main__":
    main()
