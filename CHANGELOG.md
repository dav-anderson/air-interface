# Changelog

## 0.1.0 (2026-10-24)

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
