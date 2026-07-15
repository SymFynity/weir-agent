# Releasing weir-agent

## Licence checks — read before every release

weir-agent is licensed under the Business Source License 1.1 (see
[`LICENSE`](LICENSE)). BSL has a small number of ways to go quietly wrong at
release time. None of them error; they all just silently give away more than
intended.

### Do not replace the relative Change Date with a fixed date

The licence says:

```
Change Date:          Four years from the date the Licensed Work is published.
```

This is deliberate and must stay relative. BSL applies **separately to each
version**, and each version's clock starts when *that version* is published — so
the relative form means every release automatically gets its own correct
four-year date, with no upkeep.

Substituting a fixed date (`Change Date: 2030-07-15`) looks tidier and is a trap.
That date would then apply to *every* subsequent release, so a version shipped in
2029 would convert to Apache-2.0 after one year instead of four. The error is
invisible until it has already happened, and it cannot be undone.

### Do not bump `Licensed Work` per release

```
Licensed Work:        weir-agent Version 0.2.0 or later.
```

The `or later` covers all future versions. This line changes only if the
licensing policy itself changes — it is not a version-bump chore.

### Bump the copyright year in January

Stale years are cosmetic, not fatal — but they are the first thing a reviewing
solicitor notices.

### Confirm the LICENSE ships with every artifact

BSL: *"You must conspicuously display this License on each original or modified
copy of the Licensed Work."* A published binary is a copy.

weir-agent has no Dockerfile today. If one is added, or any other distribution
channel (tarball, crates.io, package repo), it must ship `LICENSE` alongside the
binary — see `weir-proxy/Dockerfile` for the pattern.

### Check new dependency licences

weir-agent is distributed as a compiled binary, so a copyleft dependency pulled
into the tree becomes a licensing problem for the whole artifact — BSL does not
override an upstream GPL obligation. Worth a look whenever `Cargo.lock` gains
entries:

```bash
cargo tree --format '{p} {l}' | grep -viE 'MIT|Apache-2.0|BSD|ISC|Unicode|Zlib' | sort -u
```

## Version history and licensing

| Version | Licence |
|---|---|
| 0.1.0 | Apache License 2.0 |
| 0.2.0 onward | Business Source License 1.1 → Apache-2.0 after four years |

weir-agent 0.1.0 was published publicly under Apache-2.0. Those terms are
irrevocable for that version and anyone who obtained it — that is expected and
fine, not a leak to be plugged.

## Secrets check before publishing

This repo went public once already. `weir-agent.example.env` is a tracked file
whose placeholder was previously replaced in a working tree with a live-looking
org key; it was caught before it was committed. Publishing exposes *history*, not
just the current tree, so any future private→public transfer of a repo must gate
on a full history scan, not a glance at `git status`:

```bash
git log -p --all -- weir-agent.example.env | grep -iE 'weir_[A-Za-z0-9]{8,}'
```

## If the licence is ever changed again

Publishing is one-way. Any version already distributed keeps the terms it was
distributed under, permanently. While SYMFYNITY LIMITED is the sole copyright
holder, relicensing is a one-file change — that stops being true with the first
external contribution, since contributors gain copyright in their contributions.
If a CLA is wanted, it needs to be in place *before* contributions are accepted.
