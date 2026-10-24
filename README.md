# air-interface

The code that operates the programs an AIR runs, once. Two operators look after
those programs: **spi** (a Service Point Instance: bitcoind, the AIR server,
psp-mint, esp-mint) and **aso** (an AIR Service Operator: the AIR server,
fsp-mint, ecash-wallet). Both install programs from releases, check their
checksums, upgrade them one at a time with a rollback, schedule an upgrade for
the night, and keep their logs under a size cap. That code is this crate. It
knows no program and no settings file: everything that differs between the two
operators is passed in through two small traits.

**These operator interfaces are designed to run on a Linux system.** macOS is
supported only as a development machine, so that the operators can be tried and
tested on a Mac. Nothing here is built to make macOS a production system.

Version 0.1.0. How the crate is cut, and why, is in [DESIGN.md](DESIGN.md).

## Using it

```toml
air-interface = { git = "https://github.com/dav-anderson/air-interface.git", tag = "v0.1.0" }
```

It uses the `curl` and `tar` of the machine for downloads (no HTTP crate, so an
operator keeps the HTTP client it already has). Anything else an operator runs,
such as spi's `gpg` for Bitcoin Core, it runs itself.

## The two traits

**`Operator`** is one operator. spi implements it on its `Spi` settings, aso on
its `Aso`. It gives the shared code the operator's name (`NAME`: `spi`) and
settings file (`CONFIG_NAME`: `spi.toml`), its folder, home folder and
platform (`dir`, `home`, `mac`), the log and service-manager folders (`log_dir`,
`unit_dir`), where releases are listed and how many downloads to keep
(`release_url`, `release_keep`), and the scheduled-upgrade job (`UPGRADE_JOB`:
the unit name, the launchd label, the three templates, the sentence for "every
program", the log's name).

**`Managed`** is one program an operator looks after. The operator's program
enum implements it, with `type Op: Operator`. Required: `all`, `name`,
`section` (its settings section), `publishes` (whether upstream publishes
release binaries), `tags` (which tags count as versions: `Tags::Strict` is
`x.y.z`, `Tags::Loose` also takes `x.y`), `installed_path`, `bin`, `pin`,
`installed_version`, `reported`, `restart`, `service_name`, `record` and `say`.
With defaults: `managed`, `own_build`, `is_service`, `prints_version`,
`serving`, `not_serving`, `after_start` and `parse`. `say` answers the few
sentences whose wording is the operator's own (`Say::Plan`, `Say::Done`,
`Say::NotInstalled`, ...), so each operator's output stays what it was.

## What is in it

| Module | What it does |
|---|---|
| `github` | The real release source: `curl` lists a repository's releases and downloads the archive for this machine, checks its SHA-256 against the release's `SHA256SUMS`, unpacks it with `tar`. Pre-releases are never offered. |
| `release` | `Source` (tags and downloads), `check` (`upgrade check`), the pins that hold a program back, and the reset of a pin in the settings file with a backup. |
| `upgrade` | The engine: the target of each program, the verified build, the swap with a `.previous`, the restart, the wait until the program is serving, and the rollback. |
| `schedule` | `--at` / `--in`: the time, the unit and timer (Linux) or plist (macOS), the pending record, `status`, `cancel` and what the scheduled run decides from the clock. |
| `logs` | Keeping a log under a cap: move it aside (`rotate`), or copy and empty it in place while a program writes to it (`rotate_running`), and `Log`, a log an operator writes itself. |
| `template` | The `{{placeholder}}` expander for service files, and the rule for what may go into one. |
| `air` | The AIR server's environment, its nginx block and its health answer (status 200 and the body `ok`). |
| `record`, `files`, `version`, `exec` | The record of what was installed, files written whole and at once, versions and the `versions` lines, and the runner that every external program goes through (a trait, so it can be faked). |

## Tests

`cargo test`. The tests use a stub operator and a stub program (`tests/common`),
so the shared code is tested without spi or aso. `curl` is faked; `tar` is the
system's, used to build and unpack an archive. spi's and aso's own tests keep their wording, their
restart commands, their prompts and their settings.
