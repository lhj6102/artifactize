# Review notes for the tidy specification

## Open questions

1. Does tidy follow symbolic links?
   Answer: no. Links are skipped (see "Selecting old files").
2. What happens when `archive/` already holds a file with the same name?
   Answer: the file stays in place and counts as skipped (see "Moving files").

Sign off when every question has an answer that the specification supports.
