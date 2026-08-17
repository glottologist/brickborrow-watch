# brickborrow-watch

Brick Borrow lends LEGO® sets, and the sets worth having tend not to sit on the
shelf for long. This watcher polls the catalogue on a schedule and tells you the
moment a set becomes borrowable, so that you learn of a return within half an
hour rather than on your next idle visit to the site.

## Why it reads an API rather than the page

The obvious approach — fetching `https://www.brickborrow.com/setsonstock?page=14`
and parsing the HTML — does not work, and understanding why saves a great deal of
fruitless effort. Brick Borrow is a Wix site, so that URL returns an application
shell of roughly 700 KB in which no set appears; the listing is assembled in the
browser. The page code then checks that the visitor is a signed-in member holding
a particular badge and redirects everyone else to `/permissions-error`. A naive
scraper would therefore need a real browser and a logged-in session, and would
break whenever the layout shifted.

Underneath that page, however, sits an ordinary Wix data collection,
`Stores/Products`, which is readable by site visitors. We ask
`/_api/v1/access-tokens` for an anonymous visitor token, present it to
`/_api/cloud-data/v1/wix-data/collections/query`, and receive the catalogue as
clean JSON: name, availability, ribbon, and product page URL for every item. At
the time of writing this returns 1,127 products, of which 414 are in stock, and
the whole read costs twelve requests and about thirteen seconds. No account
credentials are involved at any point, and nothing in the watcher depends on the
site's markup.

Two consequences follow. The first is robustness: a visual redesign cannot break
us, and the data we diff is the same data the page renders. The second is a
maintenance risk worth naming plainly — the endpoint is Wix's internal one, so
should Wix change its shape, the watcher will fail loudly (a non-zero exit and a
message in the journal) rather than quietly report an empty catalogue.

## What counts as an alert

Every run compares the live catalogue against a snapshot of everything seen
before, and two situations raise an alert:

- **Back in stock** — a known set whose availability has flipped from out of
  stock to in stock. This is the case that matters most, since it means a set has
  just been returned.
- **New listing** — a product id that has never been recorded before, labelled
  according to whether it is available immediately or carries the site's
  `Borrow After 3 Months` ribbon.

Sets going *out* of stock are recorded silently; being told that someone else
borrowed something is of no use to anybody. Products that vanish from the
catalogue are retained in the snapshot, so a set that is briefly delisted and
then restored does not masquerade as a new arrival.

The first run has nothing to compare against, so it seeds the snapshot and sends
nothing. Were it to alert, it would announce all 1,127 products at once.

## Installation

The flake provides the binary, a development shell, and a Home Manager module
that wires up a user-level systemd timer.

With Home Manager, add the flake as an input and enable the service:

```nix
{
  inputs.brickborrow-watch.url = "path:/path/to/brickborrow-watch";

  # ... within your Home Manager configuration:
  imports = [ inputs.brickborrow-watch.homeManagerModules.brickborrow-watch ];

  services.brickborrow-watch = {
    enable = true;
    interval = "30m";
    settings = {
      ntfy = {
        enabled = true;
        topic = "brickborrow-CHANGE-ME-a7f3c9";
      };
      email = {
        enabled = true;
        smtp_host = "smtp.fastmail.com";
        smtp_port = 587;
        username = "you@example.com";
        from = "Brick Borrow Watch <you@example.com>";
        to = "you@example.com";
      };
    };
    environmentFile = "%h/.config/brickborrow-watch/secrets.env";
  };
}
```

The `settings` attribute is rendered to TOML in the Nix store, which is
world-readable; secrets must therefore go in `environmentFile` instead, as
described below. If the configuration itself is sensitive, write it by hand and
point `configFile` at it.

Without Home Manager, build the binary and drive it from a plain user timer:

```bash
nix build .#brickborrow-watch      # binary at ./result/bin/brickborrow-watch
mkdir -p ~/.config/brickborrow-watch
cp config.example.toml ~/.config/brickborrow-watch/config.toml
```

```ini
# ~/.config/systemd/user/brickborrow-watch.service
[Unit]
Description=Check Brick Borrow for newly available sets

[Service]
Type=oneshot
EnvironmentFile=%h/.config/brickborrow-watch/secrets.env
ExecStart=%h/.local/bin/brickborrow-watch
```

```ini
# ~/.config/systemd/user/brickborrow-watch.timer
[Unit]
Description=Poll Brick Borrow for newly available sets

[Timer]
OnBootSec=2m
OnUnitActiveSec=30m
Persistent=true

[Install]
WantedBy=timers.target
```

Then `systemctl --user daemon-reload && systemctl --user enable --now
brickborrow-watch.timer`. `Persistent=true` matters on a laptop: it makes systemd
run a poll that was missed while the machine was asleep.

## Configuration

The watcher reads `$XDG_CONFIG_HOME/brickborrow-watch/config.toml` unless
`--config` says otherwise; `config.example.toml` documents every field. Two
channels are available and both may run at once, in which case an alert counts as
delivered if either succeeds.

**ntfy** needs only a topic. Anyone who knows a topic name can read it, so choose
something unguessable, then subscribe from the ntfy app or
`https://ntfy.sh/<topic>` in a browser.

**Email** is sent over SMTP with STARTTLS. The password is never written to the
configuration file: the watcher reads it from the environment variable named by
`password_env`, which systemd supplies from `environmentFile`.

```bash
install -m 600 /dev/null ~/.config/brickborrow-watch/secrets.env
printf 'BRICKBORROW_SMTP_PASSWORD=your-app-password\n' \
  > ~/.config/brickborrow-watch/secrets.env
```

Gmail and most other providers require an application-specific password here
rather than your account password.

## Everyday use

A single poll runs to completion and exits, which is what makes it a natural fit
for a timer rather than a daemon. To see what a run would do without sending
anything or touching the snapshot, use the dry run:

```bash
brickborrow-watch --dry-run
```

To confirm that the channels themselves work — that the phone is subscribed and
the SMTP password is correct — send a sample alert, which touches neither the
catalogue nor the snapshot:

```bash
brickborrow-watch --test-alert
```

State lives in `$XDG_STATE_HOME/brickborrow-watch/state.json` and is written
atomically. Deleting it re-seeds from scratch, which is the correct remedy if the
file is ever corrupted; expect one silent run afterwards.

Alerts are delivered *before* the snapshot is written. Should every channel fail,
the run exits non-zero and leaves the snapshot untouched, so the next poll will
try again with the same findings rather than losing them. Progress and failures
both land in the journal:

```bash
systemctl --user list-timers brickborrow-watch.timer
journalctl --user -u brickborrow-watch -n 50
```

## Development

`nix develop` provides the Rust toolchain, after which `cargo test` exercises the
diff logic — the part where a mistake would either spam you or, worse, stay
silent when a set you wanted came back.
