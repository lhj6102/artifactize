# Requirements for tidy

`tidy` is a small command that moves old files out of a folder.

- R1: tidy never deletes a file; it only moves files into `archive/`.
- R2: A file is old when it was last modified more than 30 days ago.
- R3: `tidy --dry-run` prints the planned moves and changes nothing.
- R4: Every run ends with one summary line that counts moved and skipped files.
