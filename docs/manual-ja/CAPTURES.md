# Captures of the manual's images

How each image in `images/` was made, so it can be made again for a new version (the window shows the version, so retake them for each release). This file is outside `images/`, so `scripts/manual-html.ts` does not copy it and it is not in the release zip.

The window is drawn by egui and looks the same on macOS and Windows apart from the title bar, so the images are taken on macOS from a `cargo build --release` and the title bar (the top 30 px) is cut off.

| Image | How |
| --- | --- |
| `window-start.png` | `ft-measure --lang ja --no-chatbox --out ft-measure --last-file <file with "vive">`, with a sender putting `/avatar/parameters/FT/In/JawOpen` on 127.0.0.1:9001 so the dot is green; the window is captured with `screencapture -l <window id>` |
| `window-measuring.png` | the same with `--window-script <script>` (`wait 2500`, `measure`, `wait 600`, `measure`, `wait 30000`), captured after the second measure |
