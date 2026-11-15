# FT Measure

A Windows app that records VRChat face tracking (FT) values per expression. It listens to VRChat's OSC output, takes the avatar parameters that start with a fixed prefix (default `FT/In/`), walks you through a list of expressions, and saves the measured values as CSV and JSON Lines.

The results are meant as material for per-device FT settings (VR headset trackers, Selfie Expression, and so on).

[日本語](#日本語)

## Usage

1. In VRChat, turn on OSC (Action Menu > Options > OSC > Enabled).
2. Change into an avatar whose Avatar Descriptor forwards the Unified Expressions keys to parameters named `FT/In/<key>` (for example `FT/In/JawOpen`) with Face Tracking > Forward to Animator.
3. Double-click `ft-measure.exe`. Check that the dot at the bottom of the window turns green when you move your face, enter a device name (for example `selfie`, `vive`) and an optional memo, and press Start.
4. Make the expression shown in the window and in the chatbox, then press Measure.

| Button | Key | What it does |
| --- | --- | --- |
| Measure | Space | Records the current values (for readings, starts and stops recording) |
| Next | Enter | Moves to the next item |
| Undo | Backspace | Removes the last measurement of the current item |

- The keys work while the FT Measure window is the active window. The window shows a warning when it is not.
- Each pose is measured 3 times. Relax your face between measurements, then make the expression again.
- For "sequence" items, move to the step shown in the chatbox, hold it, and press Measure. The steps go round 3 times.
- For readings, press Measure to start, read the passage, and press Measure again to stop.
- A beep sounds for each action. Three short beeps mean the face was moving just before you pressed Measure; you can undo and measure again.
- The chatbox is visible to other players, so use a private instance.
- The window can be shown in English or Japanese (top right). The choice is remembered.

## Output

Each session creates two files in the `ft-measure` folder next to the exe. All content is in English.

- `<name>_<date-time>_summary.csv`: per item and key, the count, median, min, max, and how many measurements were moving (for readings: samples, duration, 5th and 95th percentiles)
- `<name>_<date-time>_raw.jsonl`: every measurement and the time series of readings

The files are rewritten after every measurement, so closing the window keeps everything measured so far.

## Item list

Edit `items.txt` next to the exe to change the items. One item per line, written as `English name // 日本語の名前`. Lines starting with `interval:` are readings; lines like `sequence: name | step | step…` are sequences. The English names go into the output files.

## Options

Run `ft-measure.exe --help` for the full list. The main ones:

- `--port 9001`: the port to receive OSC on
- `--prefix FT/In/`: the parameter prefix to record
- `--reps 3`: how many times each pose is measured
- `--move-threshold 0.05`, `--move-window 0.5`: the value change and the seconds that count as "moving"
- `--no-chatbox`: do not send to the chatbox
- `--lang en|ja`: the display language

## 日本語

VRChat のフェイストラッキング（FT）の値を、表情ごとに測って書き出す Windows のアプリです。

1. VRChat で OSC をオンにします（アクションメニューの Options > OSC > Enabled）。
2. Face Tracking の Forward to Animator で、Unified Expressions のキーを `FT/In/<キーの名前>` のパラメータに出すアバターに着替えます。
3. `ft-measure.exe` をダブルクリックし、右上で「日本語」を選びます。顔を動かして下の丸が緑になるのを確かめ、機器の名前とメモを入れて「始める」を押します。
4. 窓と chatbox に出る表情を作り、「測る」（スペース）を押します。「次へ」は Enter、「取り消し」は Backspace です。キーは、この窓が選ばれているときに効きます。

書き出すファイルは英語です。`items.txt` の名前は `English // 日本語` の形で書き、窓と chatbox には選んだ言語の名前が出ます。

## License

MIT OR Apache-2.0
