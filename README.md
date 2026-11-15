# FT Measure

A Windows app that records VRChat face tracking (FT) values per expression. It listens to VRChat's OSC output, takes the avatar parameters that start with a fixed prefix (default `FT/In/`), walks you through a fixed list of expressions, and saves the measured values to one JSON Lines file.

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
| Next | D (or → or Enter) | Moves to the next item once it is measured 3 times (each step 3 times for sequences, a recording for readings) |
| Undo | A (or ← or Backspace) | Removes the last measurement of the current item |

- The keys work while the FT Measure window is the active window. The window shows a warning when it is not.
- Each pose is measured 3 times. Relax your face between measurements, then make the expression again.
- For "sequence" items, move to the step shown in the chatbox, hold it, and press Measure. The steps go round 3 times.
- For readings, press Measure to start, read the passage, and press Measure again to stop. The window and the chatbox show an example of what to read.
- A beep sounds for each action.
- The chatbox is visible to other players, so use a private instance.
- The window can be shown in English or Japanese (top right). The choice is remembered.

## Output

Each session creates one file, `<name>_<date-time>.jsonl`, in the `ft-measure` folder next to the exe. All content is in English. The file is rewritten after every measurement, so closing the window keeps everything measured so far.

- Line 1 (`"type": "session"`): `format_version` (the version of this file format, now `1`), `device`, `memo`, `started`, `prefix`, `tool_version`
- Following lines: one measurement of a pose (`"type": "pose"`, with `rep`, the `values` of every key and their `spread` just before Measure) or one sample of a reading (`"type": "sample"`, with `n` and `t`). Each line has `item_no`, `item` (the English name) and `id`

`format_version` goes up when the meaning of a key or an `id` changes or one is removed; adding keys or items keeps it.

## Items

The items are built into the exe and cannot be changed, so that every file has the same items and ids. The FT generator finds the items by `id`.

| Item | `id` |
| --- | --- |
| Close both eyes tightly | `eyes_close` |
| Open both eyes as wide as you can | `eyes_wide` |
| Squint by raising the lower eyelids (as in bright light) | `eyes_squint` |
| Raise both brows as high as you can (surprise) | `brows_up` |
| Pull the brows together and down (frown) | `brows_down` |
| Open the mouth vertically as far as you can | `jaw_open` |
| With the mouth open, stretch the lips sideways | `mouth_stretch` |
| With the mouth closed, pull the corners out and up | `mouth_smile` |
| With the mouth closed, pull the corners down | `mouth_frown` |
| Pucker the lips and push them forward | `lips_pucker` |
| Bite the teeth together and open the lips ("eee") | `lips_apart` |
| Close the mouth and puff out the cheeks | `cheeks_puff` |
| Stick the tongue out as far as you can | `tongue_out` |
| Wink: Close the right eye only / Close the left eye only | `wink_right` / `wink_left` |
| Gaze (face forward): Up / Down / Right / Left | `gaze_up` / `gaze_down` / `gaze_right` / `gaze_left` |
| Lips: Press the lips together hard ("mmm") / Roll the lips into the mouth to hide them | `lips_press` / `lips_roll` |
| Jaw (mouth slightly open): Shift right / Shift left | `jaw_right` / `jaw_left` |
| Tongue out, point the tip: Up / Down / Right / Left | `tongue_up` / `tongue_down` / `tongue_right` / `tongue_left` |
| Read aloud with a neutral face | `read_neutral` |
| Read aloud with a light smile | `read_smile` |
| Relaxed neutral face | `neutral` |

## Options

Run `ft-measure.exe --help` for the full list. The main ones:

- `--port 9001`: the port to receive OSC on
- `--prefix FT/In/`: the parameter prefix to record
- `--reps 3`: how many times each pose is measured
- `--move-window 0.5`: the seconds before Measure whose change of each key is saved as `spread`
- `--no-chatbox`: do not send to the chatbox
- `--lang en|ja`: the display language

## 日本語

VRChat のフェイストラッキング（FT）の値を、表情ごとに測って書き出す Windows のアプリです。

1. VRChat で OSC をオンにします（アクションメニューの Options > OSC > Enabled）。
2. Face Tracking の Forward to Animator で、Unified Expressions のキーを `FT/In/<キーの名前>` のパラメータに出すアバターに着替えます。
3. `ft-measure.exe` をダブルクリックし、右上で「日本語」を選びます。顔を動かして下の丸が緑になるのを確かめ、機器の名前とメモを入れて「始める」を押します。
4. 窓と chatbox に出る表情を作り、「測る」（スペース）を押します。「次へ」は D キー（→ や Enter でも可）、「取り消し」は A キー（← や Backspace でも可）です。キーは、この窓が選ばれているときに効きます。

書き出すのは `<名前>_<日時>.jsonl` の 1 つのファイルで、中身は英語です。1 行目に形の版 `format_version`、各行に項目の `id` が入ります。項目は exe に組み込みで、変えられません（`id` の一覧は英語の「Items」）。窓と chatbox には選んだ言語の名前が出ます。

## License

MIT OR Apache-2.0
