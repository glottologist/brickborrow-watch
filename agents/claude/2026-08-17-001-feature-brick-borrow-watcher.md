# Brick Borrow stock watcher

**Status**: implemented and verified against the live catalogue on 2026-08-17.

## The problem

Brick Borrow lends LEGO® sets from a catalogue that turns over as borrowers
return them. The user holds an account and wants to learn, on a schedule, when a
set becomes borrowable, rather than discovering it by chance.

## What the reconnaissance established

The starting point, `https://www.brickborrow.com/setsonstock?page=14`, is not
scrapeable in the ordinary sense, and it is worth recording why so that nobody
repeats the investigation.

1. The site is built with Wix. The URL returns a 695 KB application shell in
   which no set name appears; the listing is assembled client-side.
2. The page's Velo code (fetched from the bundler at
   `filePath_public_delimiter_pages_delimiter_g8jfe.js`) calls
   `wix-members-frontend`, checks the member's badge against
   `a1e8a52a-cd51-49ad-b41b-73aaa26f2339`, and redirects anyone else to
   `/permissions-error`. The page is therefore gated on membership, not merely on
   JavaScript.
3. The sets are nonetheless ordinary Wix Stores products. The collection
   `Stores/Products` is readable at visitor level, which means an anonymous token
   suffices:
   - `GET /_api/v1/access-tokens` yields, among much else,
     `apps["225dd912-7dea-4738-8688-4b8c6955ffc2"].instance` — the Data Binding
     app's instance token.
   - `POST /_api/cloud-data/v1/wix-data/collections/query` with that token in the
     `Authorization` header and a body of
     `{"collectionName":"Stores/Products","segment":"LIVE","query":{"paging":{...},"fields":[...]}}`
     returns the catalogue. Paging is capped at 100 items per request.
4. The catalogue on 2026-08-17 held 1,127 products, every one of brand `LEGO`,
   of which 414 were in stock. `inStock` and `quantityInStock > 0` agreed on all
   1,127 rows, so `inStock` alone is a sufficient availability signal. Two
   successive reads minutes apart differed by two items, which confirms that the
   churn the user wants to catch is real.
5. Fourteen pages of roughly thirty items each corresponds closely to the 414
   in-stock figure, which is the evidence that this collection is indeed what
   `/setsonstock` renders.

## Design decisions

**A poll-and-exit binary driven by a systemd timer, not a daemon.** The work is a
thirteen-second read every thirty minutes; a resident process would buy nothing
and would lose its state on a crash. `Persistent=true` on the timer covers the
laptop case where the machine sleeps through a scheduled run.

**A JSON snapshot rather than a database.** The state is a map from product id to
name and availability, some 1,100 entries. SQLite would add a C dependency to the
Nix build in exchange for nothing we need. The file is written to a temporary
path and renamed, so an interrupted run cannot corrupt it.

**Products are never removed from the snapshot.** Were they pruned, a set that
disappeared from the catalogue for a single poll would return as a "new listing".
Retention costs a few kilobytes and removes an entire class of false alert.

**A short read is an error, not an empty result.** `fetch_all` refuses to return
unless the number of items collected equals the `totalCount` the API reported,
and it aborts if that count changes mid-read. This is the guard that stops a
partial fetch from being interpreted as the catalogue emptying.

**Alerts are delivered before the snapshot is written.** If every channel fails,
the run exits non-zero and the snapshot stays as it was, so the next poll
rediscovers the same events. The alternative — recording first — would lose an
alert permanently on a transient SMTP failure.

**Secrets live in the environment, never in the config file.** The Home Manager
module renders `settings` into the Nix store, which is world-readable; the SMTP
password is read from the variable named by `password_env`, supplied by systemd's
`EnvironmentFile`.

## Verification performed

- `cargo test` covers the four diff cases: unseen products alert as new whatever
  their stock, a restock alerts exactly once, going out of stock is silent, and a
  delisted-then-relisted product does not re-alert as new.
- A live seeding run recorded 1,127 products (414 in stock) in 13 seconds.
- The snapshot was then edited to mark three in-stock sets as previously out of
  stock and to delete three entries; the following dry run reported precisely
  three `BACK IN STOCK` and three `NEW` events, correctly split into two
  available and one carrying the `Borrow After 3 Months` ribbon.
- A live alert was delivered to ntfy and read back from the topic, confirming
  the title, the UTF-8 body, and the truncation notice; the snapshot was written
  only after delivery, and the following run reported no events.
- `cargo clippy --all-targets` is silent and `nix flake check` passes. The
  package derivation evaluates; a full `nix build` was still fetching the Rust
  toolchain when this note was written, since a concurrent Home Manager build
  held the bandwidth.

## Known risks

The Wix data endpoint is internal to Wix and undocumented, so a change on their
side would break the read. The failure is loud — a non-zero exit and a message in
the journal — rather than a silent empty catalogue, which is the behaviour one
wants from a watcher. Should Brick Borrow restrict the collection's permissions
to members, the remedy would be to authenticate as a member and carry the session
cookie; that path was deliberately not taken while an anonymous read suffices.

## Left undone, deliberately

No watchlist filter was implemented: the user chose alerts on the whole
catalogue. Should the volume prove tiresome, the natural addition is a list of
set numbers matched against the parenthesised number in each product name, which
1,117 of the 1,127 names carry.
