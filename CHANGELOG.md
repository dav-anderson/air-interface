# Changelog

## 0.3.0 (2026-10-26)

Breaking: **0 means "keep everything"** for every count and size that caps what
the operators keep. A caller that passed 0 meaning "keep none" or "a cap of 0 MB"
now gets the opposite; spi and aso refused 0 before, so neither did.

- `release::prune`: `Operator::release_keep` of 0 removes nothing. Otherwise it
  removes in ascending version order (oldest first), skipping the newest `keep`
  and every version in `protect` (the installed release and the one before it,
  which are never removed). The returned list is in removal order, oldest first;
  it was newest first.
- `logs::prune`: `keep` 0 removes nothing. `logs::rotate` and
  `logs::rotate_running`: `max_mb` 0 never moves a log aside (and `keep` 0
  keeps every `.bak` file). `DEFAULT_KEEP` (4) and `DEFAULT_MAX_MB` (50) are
  unchanged.
- `files::prune_backups` and `files::backup_and_replace`: `keep` 0 removes
  nothing.
- Doc comments say what 0 means; `Operator::release_keep` says it.
- Tests: `release_prune_removes_the_oldest_first_and_keep_0_removes_nothing`,
  `backup_pruning_with_keep_0_leaves_every_backup` and
  `a_max_of_0_never_moves_a_log_and_a_keep_of_0_keeps_every_backup`; the stub
  operator's `release.keep` is a field (2 unless a test sets it).

## 0.2.0 (2026-10-24)

The crate is renamed **air-minterface** (it was air-interface; the repository is
now https://github.com/dav-anderson/air-minterface). Breaking for anyone who
depends on it: the package is `air-minterface`, the library `air_minterface`,
and the line `versions` shows in both operators is named `air-minterface`. No
other change.

## 0.1.0 (2026-10-24)

Released as air-interface.

First version. The code that spi and aso had twice, cut out into one crate.

- `Operator` and `Managed`, the two traits an operator and its programs implement.
- `github`: the release source through `curl` and `tar`, with the SHA-256 check;
  `release`: tags (`Tags::Strict` for `x.y.z`, `Tags::Loose` for `x.y` too), `check`,
  pins, and the reset of a pin with a backup (the newest N kept, the settings file
  replaced whole and at once); `upgrade`: the engine with its rollback;
  `schedule`: `--at`/`--in`, `status`, `cancel`; `logs`: rotation at open and in
  place; `template`; `air` (including the health answer: status 200 and the body
  `ok`); `record`; `files`; `version` (including the `air-interface` line both
  operators show); `exec`.
- Differences between the two operators that this crate settles: the reset of a
  pin writes the settings file the way aso did (backup, then whole-file replace);
  spi's install records are written whole and at once; a file made executable
  goes through one `set_mode`.
- Docs: designed to run on a Linux system; macOS is a development machine only.
