# air-minterface: design notes

**What belongs here.** Code that spi and aso both had, byte for byte or nearly.
If a function needs to know a program's name, it is passed one (`Managed`); if it
needs the operator's folders or wording, it is passed the operator (`Operator`).
Anything that is a command (the prompts, the order of questions, what is printed
around a step), anything about one program's own settings (spi's `[bitcoind]`
download with its gpg check), and the HTTP client each operator already had stay
in the operator. spi keeps `reqwest` and aso keeps its `httpc`; this crate runs
`curl` for releases, as both already did.

**Two traits, not one.** `Operator` is the operator (one value, the settings);
`Managed` is a program (an enum with a `Copy` value). The engine is generic over
`P: Managed` and reaches the operator as `P::Op`, so it takes one argument for
both. `Operator` has associated consts (`NAME`, `CONFIG_NAME`, `UPGRADE_JOB`)
for what is the same for every call, which keeps the messages the operators
print (`spi upgrade cancel`) in one place without passing strings around.

**The wording is the operator's.** Where the two operators said the same thing
differently (`spi.toml names no version` against `aso.toml names no version`, how
long a service was down, what to run as root), the engine does not choose: it
asks `Managed::say`. That was the price of moving the engine without changing a
line an operator prints.

**Hooks have defaults for the common case.** `serving` is "the health check
answered" (a mint also has to report the new version), `is_service` is true (a
wallet run on demand is not), `prints_version` is true (air_server prints none,
so only the operator's record of the exact file says what it is), `after_start`
adds nothing (aso says how long the service was down). `installed_version` is a
required hook because spi falls back to the build `bin` names and aso does not.

**Tags.** A tag is `x.y.z` for fsp-mint and the mints and may be `x.y` for
Bitcoin Core. The rule is the program's own (`Managed::tags`), so one crate does
not decide for both. A pre-release or a tag with anything but digits is never a
version.

**Files are written whole.** A settings file, a record or a pending upgrade is
written to a temporary file and renamed over the old one, so a crash leaves the
old one.

**Logs.** A program that opens its log fresh at each start is rotated by moving
the log aside (`rotate`). One that runs for months holds the file open, so while
it runs the log is copied aside and emptied in place (`rotate_running`); a rename
would leave it writing to the moved file. The in-place step assumes the writer
opened the file for appending, so its next write goes to the new end; the
operators' units open it so (systemd's `StandardOutput=append:`, or `>>` in a
shell), and this is the thing to confirm first on a real machine.

**Linux is the target.** The launchd side exists so that the operators can be
tried on a Mac. It has the same shapes as the systemd side and no more.

**What is not tested.** Nothing here runs `systemctl`, `launchctl`, a real
GitHub or a real program: the runner is a trait, `curl` is faked, and the
service files are checked as text.
