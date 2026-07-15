# Releasing symfynity-agent

## Licence checks — read before every release

symfynity-agent is licensed under the Business Source License 1.1 (see
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
Licensed Work:        symfynity-agent Version 0.3.0 or later.
```

The `or later` covers all future versions. This line changes only if the
licensing policy itself changes, or the Licensed Work is renamed — it is not a
version-bump chore. It has changed exactly once, at 0.3.0, when the agent was
renamed from weir-agent to symfynity-agent.

### Bump the copyright year in January

Stale years are cosmetic, not fatal — but they are the first thing a reviewing
solicitor notices.

### Confirm the LICENSE ships with every artifact

BSL: *"You must conspicuously display this License on each original or modified
copy of the Licensed Work."* A published binary is a copy.

symfynity-agent has no Dockerfile today. If one is added, or any other distribution
channel (tarball, crates.io, package repo), it must ship `LICENSE` alongside the
binary — see `symfynity/Dockerfile` for the pattern.

### Check new dependency licences

symfynity-agent is distributed as a compiled binary, so a copyleft dependency pulled
into the tree becomes a licensing problem for the whole artifact — BSL does not
override an upstream GPL obligation. Worth a look whenever `Cargo.lock` gains
entries:

```bash
cargo tree --format '{p} {l}' | grep -viE 'MIT|Apache-2.0|BSD|ISC|Unicode|Zlib' | sort -u
```

## Version history and licensing

| Version | Published as | Licence |
|---|---|---|
| 0.1.0 | weir-agent | Apache License 2.0 |
| 0.2.0 | weir-agent | Business Source License 1.1 |
| 0.3.0 onward | symfynity-agent | Business Source License 1.1 → Apache-2.0 after four years |

Both earlier versions were published publicly under the name weir-agent, and
0.1.0 under Apache-2.0. Those terms are irrevocable for that version and anyone
who obtained it — that is expected and fine, not a leak to be plugged. A
published version keeps the name and terms it was published under, permanently.

## Secrets check before publishing

This repo went public once already. `symfynity-agent.example.env` is a tracked file
whose placeholder was previously replaced in a working tree with a live-looking
org key; it was caught before it was committed. Publishing exposes *history*, not
just the current tree, so any future private→public transfer of a repo must gate
on a full history scan, not a glance at `git status`:

```bash
git log -p --all --follow -- symfynity-agent.example.env weir-agent.example.env \
  | grep -iE '\b(sfy|weir)_[A-Za-z0-9]{8,}'
```

Two details that are easy to get wrong, and both make the scan silently pass:

- **Scan the old filename too.** History under `weir-agent.example.env` predates
  the 0.3.0 rename; without `--follow` and the old path, the commits that matter
  are the ones you don't see.
- **Scan the old key prefix too.** Org keys are `sfy_` from now on, but every key
  that ever existed while this repo was public was `weir_`. A scan for the
  current prefix alone would have missed the exact incident that prompted
  this section.

## If the licence is ever changed again

Publishing is one-way. Any version already distributed keeps the terms it was
distributed under, permanently. While SYMFYNITY LIMITED is the sole copyright
holder, relicensing is a one-file change — that stops being true with the first
external contribution, since contributors gain copyright in their contributions.
If a CLA is wanted, it needs to be in place *before* contributions are accepted.
