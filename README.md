# ParadigmShift

A clean, film-editor style app for designing and running **fMRI paradigms** — written in Python with Qt (PySide6).

Build your run like a video edit: drag blocks onto a timeline, trim them to whole TRs, drop in pictures, preview
exactly what the participant will see, then press **Run in scanner**. The app waits for the
**NordicNeuroLab SyncBox** trigger and records **NNL ResponseGrip** presses and picture ratings into BIDS-style
`events.tsv` files.

![Editor](docs/screenshots/02_editor_rating_selected.png)

## Start

```bash
pip install -r requirements.txt
python -m paradigmshift                                         # empty project
python -m paradigmshift examples/emotional_rating.paradigm.json # example
```

## Paradigm modules

Start from a classic task with **▦ Templates** (Ctrl+T), set the numbers, and get a complete, editable
timeline. Ready-made projects are in `examples/` (File → Open example).

Ten modules, grouped by research area:

| Module | Default design | What is logged |
|---|---|---|
| **Emotion & reward** | | |
| Hariri face matching (Hariri 2002) | S F S F … S blocks, 6 matching trials × 5 s each; angry/fearful faces vs. shapes · 4:48 min | left/right, RT, correct |
| Emotion regulation (Ochsner 2002/04) | cue LOOK/DECREASE (opt. INCREASE) → picture 7 s → rating → jittered relax | rating, RT |
| Monetary incentive delay (Knutson 2000/01) | cue (win/lose/nothing) → anticipation 2–2.5 s → target 0.3 s → feedback → ITI; 15 trials per condition | hit/miss, RT, feedback shown |
| **Cognitive control** | | |
| Go / No-Go | 160 letters, 25 % No-Go (X), never two No-Go in a row | hit, miss, false alarm, correct rejection |
| Stroop | colour words in ink, 50 % incongruent (opt. neutral XXXX), 4 colours → 4 grip buttons, jittered ITI | correct/incorrect, RT per condition |
| Eriksen flanker | arrows, 50 % incongruent (opt. neutral), left/right grip, jittered ITI | correct/incorrect, RT per condition |
| **Working memory** | | |
| N-back | blocks of 0-back and 2-back (any levels, e.g. "0, 1, 2"), 15 letters, 33 % targets, cue + rest | hit, miss, false alarm, correct rejection |
| **Localizers & rest** | | |
| Motor localizer | finger tapping LEFT / RIGHT (opt. BOTH) 20 s vs. rest 20 s, 5 cycles | taps per 2-s window (compliance) |
| Visual checkerboard | 8 Hz flickering radial checkerboard 20 s vs. grey 20 s, red fixation dot | – |
| Resting state | instruction, then 8 min fixation (eyes open) or empty screen | – |

These rely on several block types that you can also use on their own in any paradigm:

* **Matching trial** – three pictures (target on top, two choices below). You set the correct side and which
  grip buttons mean left and right.
* **Go / No-Go trial** – a letter or picture that is visible for a set time, followed by a fixation symbol. No-Go
  trials are shown in red on the timeline.
* **Choice trial** – a word, symbol or picture in any colour, plus the correct button, *any* button (hit) or
  *no press* (withhold). Used by Stroop, flanker, N-back and the MID target.
* **Feedback** – shows one text if the previous trial was done right and another if not (MID).
* **Flickering image** – any image block can alternate with a second picture at a set rate (checkerboard).

**Stimuli:** licensed sets such as NimStim, KDEF, IAPS or OASIS can't be redistributed, so the templates
generate *placeholder* stimuli: schematic angry/fearful faces, geometric shapes, and abstract neutral/negative
scenes. For real studies, point the template at your own folders:
* the Hariri face folder is sorted into angry and fearful faces by file name (`angry`/`fear`, NimStim
  `_AN_`/`_FE_`, KDEF `AN`/`AF`);
* the emotion-regulation template takes one folder of neutral and one of negative pictures.

The defaults follow the published protocols, but check them against the protocol you are replicating.

## How it works – at a glance

| Area | What you do there |
|---|---|
| **Blocks** (left) | Drag *Fixation, Image, Rating, Instruction, Pause, Response window* onto the timeline – or double-click to append. |
| **Pictures** (left) | Import pictures or drop folders; drag one or many onto the timeline (each becomes an image block). |
| **Participant view** (centre) | Shows the screen at the playhead. ▶ / Space plays a preview of the run. |
| **Inspector** (right) | Edit the selected block. With nothing selected: TR, dummy scans, background, run summary. |
| **Timeline** (bottom) | Ruler in time *and* volumes. Drag to move, drag edges to trim, everything snaps to the TR grid and to block edges (hold **Alt** to place freely). Track **V1** is what's on screen, **R2** records grip presses. |
| **▦ Templates** | Hariri, Go/No-Go, emotion regulation – see *Paradigm modules* above. |
| **✦ Quick design** | Builds a whole block design (ON/OFF, alternating or randomised) or an event-related design with jittered ITIs and optional ratings. |
| **Hardware** | Configure and *live-test* the SyncBox and grips (lamps light up, measured TR is shown, built-in scanner simulator). |
| **Run in scanner** | Participant ID, run, screen → full-screen presentation that waits for the trigger. |

Right-click anything on the timeline for more (duplicate, repeat ×N, split at playhead, close gaps…).
F1 lists the keyboard shortcuts.

## During a run: control room & participant screen

**Run in scanner** opens two windows:

* **Participant screen** (projector): shows only the paradigm. Before the start – including the
  **dummy scans** – the participant sees a fixation cross, an empty screen or a text of your choice. After the
  run they see an end text. Trigger counts and other technical information never appear here.
* **Control room** (operator monitor):
  * a live mirror of the participant screen;
  * status: waiting → dummy scans *n / N* → running → finished;
  * elapsed and remaining time, volume and trigger counts, and the measured TR;
  * a progress strip of the whole run, with the current and next block;
  * a live list of answers (RT, correct/incorrect, hit/miss/false alarm, ratings);
  * a running performance summary, e.g. accuracy per condition, hit/false-alarm rate, mean rating per
    condition;
  * warnings when the participant stops answering, when no trigger arrives, or when another app has the
    keyboard focus (SyncBox keys would be lost).

  Every section can be **folded** by clicking its title or via **☰ Sections**: status, run progress,
  participant mirror, now & next, performance and answers. A folded section keeps a one-line summary in
  its title bar, and the folding is remembered for the next run.

  Aborting takes two clicks or two presses of Esc (configurable). After an abort, the clock stops, the
  control room clearly shows *ABORTED at mm:ss*, the part of the run that never happened is hatched, and the
  participant sees an empty screen.

You set the number of dummy scans in the run dialog or in the inspector (nothing selected). The paradigm clock
starts with trigger *N + 1*. In the **Matching trial** a frame marks the picture the participant chose (colour
adjustable, can be switched off).

## Task blocks (library tab “Task blocks”)

Next to the basic blocks, the library has ready-made blocks for the standard paradigms. Drag them onto the
timeline or double-click them. If a block has variants, a small menu asks which one you want.

* **Task trials:**
  * *Stroop trial*: choose the word and ink colour.
  * *Flanker trial*: choose the arrow direction and the flankers.
  * *N-back letter*: choose the letter, whether it is a target, and the level.

  The inspector shows these task settings. Changing them updates the text, colour, correct button and
  condition automatically, also for many selected trials at once. Trials generated by the templates are
  task trials too.
* **Task sequences** insert several blocks at once (as one undo step), moving later blocks on all tracks:
  * *MID trial* (win / avoid losing / nothing);
  * *Emotion-regulation trial* (LOOK / DECREASE / INCREASE);
  * *N-back block* (0-, 1-, 2- or 3-back);
  * *Hariri block* (faces / shapes);
  * *Tapping block* (left / right / both, with response windows);
  * *Checkerboard*;
  * *Resting state*.

  Right-click on the timeline → *Add block here → Task blocks* works too.

## Editing many blocks at once

* **Select several blocks of the same type**, e.g. all ratings. You can do this in three ways:
  * right-click a block → *Select all Rating blocks*;
  * **Edit → Select…** (by type or condition);
  * in the inspector, *Edit all … blocks together*.

  Every setting in the inspector then applies to all selected blocks, for example *Answer with: one grip
  button per value / cursor ◀ ▶*. Settings where the blocks differ are marked **≠**. Fields you don't touch
  stay as they are. One Ctrl+Z undoes the whole change.
* **Edit → Find and replace text** (Ctrl+H) changes words or sentences in all instructions, rating questions,
  rating labels and block labels at once.

## Unpleasant pictures

**Options → Blur picture previews** (Ctrl+Shift+B, also in Preferences) blurs every picture preview in the
editor: timeline thumbnails, picture bin, inspector and participant-view preview. A separate preference
blurs the control-room mirror. The participant's screen always shows the real pictures.

## Options & preferences

The **Options** menu holds quick switches (snapping, hints, how new ratings are answered, two-step abort, saving
the paradigm with each run). **Options → Preferences…** (Ctrl+,) has the rest:
* text size, opening the last file on start, autosave;
* defaults for new paradigms (TR, dummy scans, background, waiting and end screen) and new blocks;
* snap distance;
* default data folder;
* control room warnings (after how many trials without an answer, after how many TRs without a trigger) and
  its refresh rate.

## Scanner hardware (NNL)

* **SyncBox** – *USB keyboard mode* (default): the trigger arrives as a key, default `s`.
  *Serial mode* follows **NordicNeuroLab's own serial protocol** (from NNL's
  [py_syncbox](https://github.com/nordicneurolab/py_syncbox)): 57600 baud, trigger `s`, grips `a`–`d`.
  * **🔍 Auto-detect SyncBox** sends NNL's computer-mode command to every serial port. The SyncBox answers it,
    so it is found without the scanner running. Port, baud rate, trigger `s` and grip keys `a`–`d` are set.
    If no box answers, the app falls back to listening for a device that sends on its own.
  * With **Control the SyncBox from ParadigmShift** (on by default), each run switches the SyncBox to computer
    mode and configures it with the paradigm's TR and number of volumes (+ dummy scans + margin). Then it
    starts in synchronisation mode. After the run the box is stopped and returned to manual mode.
  * **Simulate scanner** then uses the SyncBox's own simulation, which tests the real cable path. The same
    happens in the run dialog's *Simulate scanner*.
  * Errors say what to do: port not found, port in use (e.g. by NNL software), Linux `dialout` permission,
    and "the SyncBox did not confirm …".
* **ResponseGrips** – four named buttons. Default keys are `1`–`4` (keyboard mode). For serial mode,
  *Use NNL serial keys (a – d)* sets them, which auto-detect also does. Rename, add, or re-learn keys in the
  Hardware dialog.
* **Rating scales** – *direct* (each grip button = one value) or *cursor* (move ◀ ▶, confirm ✓).
* **Simulate scanner** – test any paradigm on a laptop; triggers are generated at the TR.

## Output

For each run (in the chosen folder):

* `sub-01_task-<name>_run-01_events.tsv` – BIDS-style: `onset`, `duration`, `trial_type` (= condition),
  `stim_file`, `response`, `response_time`, `rating`, `correct`, `outcome` (Go/No-Go: hit, miss, false_alarm,
  correct_rejection) and `planned_onset` for timing checks.
* `sub-01_task-<name>_run-01_log.tsv` – every trigger and button press with its time.

Onsets are relative to the first trigger after the dummy scans. **Export onsets** writes the planned design as
`events.tsv` plus FSL 3-column files per condition.

> Timing note: stimuli are switched by a 1 ms precision timer and onsets are logged when the switch happens;
> drawing is not locked to the display refresh. For publication-grade timing, verify with a photodiode.

Paradigms are saved as `*.paradigm.json` (picture paths stored relative to the file).

## Screenshots

| | |
|---|---|
| ![](docs/screenshots/01_empty_start.png) First launch – the empty timeline tells you what to do | ![](docs/screenshots/03_editor_image_zoomed.png) Zoomed in: picture thumbnails, conditions, response windows |
| ![](docs/screenshots/04_editor_paradigm_settings.png) Nothing selected → paradigm settings & summary | ![](docs/screenshots/07_quick_design.png) Quick design wizard |
| ![](docs/screenshots/05_hardware_syncbox.png) SyncBox setup with live trigger monitor | ![](docs/screenshots/06_hardware_grips.png) ResponseGrips with live button lamps |
| ![](docs/screenshots/08_run_dialog.png) Run setup | ![](docs/screenshots/10_participant_rating.png) Participant screen: rating trial |
| ![](docs/screenshots/11_templates_hariri.png) Templates: Hariri | ![](docs/screenshots/12_templates_emoreg.png) Templates: emotion regulation |
| ![](docs/screenshots/13_hariri_editor.png) Hariri in the editor | ![](docs/screenshots/14_gonogo_editor.png) Go/No-Go – No-Go trials in red |
| ![](docs/screenshots/15_emoreg_editor.png) Emotion regulation: cue → picture → rating → relax | ![](docs/screenshots/16_participant_hariri.png) Participant screen: Hariri trial, chosen face framed |
| ![](docs/screenshots/18_control_room_hariri.png) Control room during a Hariri run | ![](docs/screenshots/19_control_room_gonogo.png) Control room during Go/No-Go |
| ![](docs/screenshots/17_control_room_dummy_scans.png) Control room during dummy scans | ![](docs/screenshots/09_participant_waiting.png) …while the participant only sees the fixation cross |
| ![](docs/screenshots/23_control_room_aborted.png) Aborted run: clock stopped, rest hatched | ![](docs/screenshots/20_bulk_edit_ratings.png) All 8 rating blocks edited together (≠ = different values) |
| ![](docs/screenshots/21_find_replace.png) Find and replace in all texts | ![](docs/screenshots/22_preferences.png) Preferences |
| ![](docs/screenshots/24_templates_stroop.png) Template list by category: Stroop | ![](docs/screenshots/25_templates_mid.png) Monetary incentive delay template |
| ![](docs/screenshots/26_mid_editor.png) MID in the editor: cue → anticipation → target → feedback | ![](docs/screenshots/27_nback_editor.png) N-back in the editor |
| ![](docs/screenshots/28_participant_stroop.png) Stroop trial | ![](docs/screenshots/29_participant_flanker.png) Flanker trial (incongruent) |
| ![](docs/screenshots/30_participant_mid_feedback.png) MID feedback after a hit | ![](docs/screenshots/31_participant_checkerboard.png) Visual checkerboard |
| ![](docs/screenshots/34_blurred_previews.png) Blurred previews (emotion regulation) | ![](docs/screenshots/36_control_room_folded.png) Control room with folded sections |
| ![](docs/screenshots/32_task_blocks_stroop.png) Task blocks tab; a Stroop trial with its task settings | ![](docs/screenshots/33_task_blocks_sequences.png) Resting + 3 MID trials + tapping block, inserted from the library |

Screenshots are generated with `QT_QPA_PLATFORM=offscreen python tools/screenshots.py`.

## Development

```bash
pip install -e ".[test,serial]"
QT_QPA_PLATFORM=offscreen pytest
python tools/make_example.py     # regenerates examples/
```

Code layout: `model.py` (paradigm, blocks, generators – no Qt), `templates.py` (Hariri, Go/No-Go, emotion
regulation), `placeholders.py` (generated stimuli), `timeline.py`, `panels.py`, `dialogs.py`,
`runner.py` (run clock, logging, participant screen), `controlroom.py` (operator window), `prefs.py` (preferences), `presets.py` (task blocks), `hardware.py` (SyncBox/grips input), `stimulus.py` (shared renderer).
