# tidy specification

`tidy [--dry-run] FOLDER` tidies one folder. It does not descend into
subfolders.

## Selecting old files

R2: tidy reads each regular file's modification time. A file is old when that
time is more than 30 days (2,592,000 seconds) before the start of the run.
Symbolic links and subfolders are skipped.

## Moving files

R1: tidy creates `FOLDER/archive/` when it is missing and renames each old file
into it. It never deletes or overwrites a file. When `archive/` already holds a
file with the same name, the file stays where it is and counts as skipped.

## Dry run

R3: With `--dry-run`, tidy prints one `would move NAME` line per old file and
makes no change to the folder, not even creating `archive/`.

## Summary line

R4: The last line of every run is `moved N, skipped M`, including dry runs,
where N counts the planned moves.

## Diagram

`diagram.png` shows the three stages from left to right: scan (blue), select
(orange) and move (green).
