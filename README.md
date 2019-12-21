# fedora-update-notifier

This is a small program that checks for fedora updates from the `updates-testing` repository that are installed on the
current system, and creates a click-able desktop notification (via the `D-Bus` interface for notifications) that takes
the use to a page in the bodhi web interface where they can leave feedback for these updates.

This program could be automated to run at regular intervals - for example, with an autostart entry to run at login, or
with a systemd user session (timer) unit.

### requirements

The program assumes that the `dnf` and `rpm` binaries are present on the system (which is probably a reasonable
assumption for a CLI tool targeted at fedora users).

It also expects the FAS username of the current user being stored in a configuration file at `~/.config/fedora.toml`,
with these contents:

```toml
[FAS]
username = "FAS_USERNAME"

[fedora-update-notifier]
interests = ["package1", "package2"]
```

This value is used to filter out updates that the user themselves has submitted, or has already commented on.

### installation

To compile the program, first install `cargo` and `dbus-devel`.

Download the sources, and easily build and install the binary for yourself:

```console
$ git clone https://github.com/ironthree/fedora-update-notifier.git
$ cd fedora-update-notifier
$ cargo install --path .
```

### usage

The compiled program is located in `~/.cargo/bin`. To be able to run it with ease, add `$HOME/.cargo/bin` to your `$PATH`, e.g. by editing `~/.bash_profile`.

Without that, use the following to run the app:

```console
$ ~/.cargo/bin/fedora-update-notifier --help
```
