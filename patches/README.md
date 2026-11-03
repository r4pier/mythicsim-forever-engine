# patchsim: sim patch notes before MythicSim updates

`patchsim` runs a character twice on this engine: once unchanged (baseline) and once with your
changes from `patches/overrides.json`. Both runs use the same seed, and it prints the difference.
It exists for the day or two between a balance patch and MythicSim's own update. After that,
use MythicSim.

This is fork-only tooling. The engine's own `forever-engine` command, its data and its tests are
unchanged: without an overrides file installed, nothing here runs.

## Setup (Windows 11, macOS, Linux)

Install Rust from https://rustup.rs. The repository pins its toolchain in `rust-toolchain.toml`,
and rustup installs that version the first time you build. Then build once from the repository
folder:

```powershell
cargo build --release
```

The first build takes about five minutes. After that, run:

```powershell
.\target\release\patchsim.exe presets
.\target\release\patchsim.exe run --preset frost-mage
```

On macOS and Linux it is `./target/release/patchsim`. `cargo run --release --bin patchsim -- run
--preset frost-mage` works everywhere too.

Changing `overrides.json` needs no rebuild. Rebuild only after pulling new engine code.

## Web page

```powershell
.\target\release\patchsim.exe serve
```

This opens http://127.0.0.1:8080/ in your browser (`--port N` picks another port, `--no-open`
skips the browser). The page works like MythicSim's sim page: pick class, spec and race, the fight
length and targets, click talents in the trees, change reference gear and enchants by slot, and
open Advanced settings for the fight, the boss, the rotation, buffs, debuffs and consumables.
Under Rotation, "Custom APL" takes the rotation JSON exported from MythicSim's rotation editor.
The Patch overrides panel edits the same changes as `overrides.json` (Import file and Save file
move them in and out), and Run compares baseline and patched DPS, ability by ability.

The server only listens on your own machine and stops with Ctrl+C. Each run goes through
`patchsim run`, so the page and the command line always give the same numbers. Overrides and
custom rotations are remembered in the browser. After an upstream sync that changes talents,
rerun `python3 tools/web_talents.py` and rebuild so the trees match.

## Workflow after a patch

1. Read the notes and find each spell: `patchsim spell "Frostbolt"` lists every rank with its
   ID, level and effects. `patchsim spell 10181` prints one row in full, including a talent's
   per-rank `TalentCurve`.
2. Write the changes into `patches/overrides.json`, one entry per line of the notes.
3. Run `patchsim run --preset <spec>`, or `--request <your character>.json`.
4. Read the **Changes** block at the bottom. Every change should say what it touched. A change
   that `MATCHED NOTHING`, or a warning that a spell's output did not move, means the engine
   models that number somewhere else. Try another kind of change (below).

Example output:

```
frost-mage  |  10000 fights of 120 s  |  overrides: patches/overrides.json

                  baseline     patched    change
DPS                  510.9       548.8    +7.42%
TPS                  534.3       573.0    +7.24%

action (damage+healing per s)                   baseline   patched    change    casts
Frostbolt Rank 10 (10181) *                        379.2     417.1   +10.00%     40.6
...
Changes
  #1 Frostbolt: damage x1.1: changed 11 prepared field(s)
```

## The overrides file

```json
{
  "label": "Patch 1.60.2, 6 November",
  "changes": [
    { "spell": "Frostbolt", "damage": "+10%", "note": "Frostbolt damage increased by 10%" },
    { "spell": "Ice Lance", "cooldown": "-2s" },
    { "spell": "Critical Mass", "curve": 0, "value": "x1.5", "disabled": true }
  ]
}
```

`spell` is a name, which covers every rank, an ID, or a list of IDs. `note` is for you.
`"disabled": true` keeps an entry in the file without applying it. `patches/examples/every-kind.json`
has one of each kind of change.

### Values

| Write | Means |
| --- | --- |
| `"+10%"`, `"-15%"` | multiply by 1.10, 0.85 |
| `"x1.5"` | multiply by 1.5 |
| `"+5"`, `"-2s"` | add (an `s` means seconds, for `cooldown` and `cast_time`) |
| `"=8s"`, `30`, `[2, 4, 6]` | set |

### Kinds of change

| Key | What it changes | Use for |
| --- | --- | --- |
| `damage` | the spell's damage multiplier (direct and periodic) | "X deals 10% more damage" |
| `healing` | a heal's amount, its tick and its absorb | "X heals for 10% more" |
| `coefficient` | the spell power coefficient (direct and periodic) | "X's spell power scaling increased" |
| `cooldown` | the cooldown, in seconds or percent | "X's cooldown reduced to 8 sec" |
| `cost` | the base mana, rage or energy cost | "X costs 10% less mana" |
| `cast_time` | the base cast time, in seconds | "X's cast time reduced by 0.5 sec" |
| `row` + `value` | a field in the client spell row, before preparation | base points, durations, proc chances, debuff sizes |
| `curve` + `value` | a talent's per-rank values (`true` for every effect, or an effect index) | "Talent now gives 3/6/9%" |
| `prepared` + `value` | any field of the prepared spell (`bonus_crit_percent`, `dot/base_tick_count`) | anything else the engine exposes |

The first six act on the prepared character, after talents and gear are worked out. They are
the most reliable choice and match patch notes that state a final change.

`row` and `curve` act while the engine reads the client data, so the change flows through
everything the engine derives from it. Use them for talent values, debuff sizes and base
points. Field names come from `patchsim spell <id>`. Watch the effect index: Frostbolt's
`Effects/0` is the slow and `Effects/1` the damage. Changing `BasePoints` moves only the base
damage, not the spell power part. A curve must keep the number of ranks the data has.

What overrides cannot do: add a mechanic the engine does not have. A patch that adds a talent
or changes how a proc works needs Rust code in `src/classes/<class>/`, and the engine refuses
a character that uses a mechanic it cannot simulate exactly.

## Characters

`patches/presets/` has one character per spec, taken from MythicSim's own starter presets and
healer fixtures in this repository. `patches/presets/SOURCES.json` lists where each one came from.

The talent trees changed with hotfix 113554 (30 October). The old Mage, Feral Druid and hybrid
Paladin preset strings no longer load, so their talents were remapped by talent name onto the
current trees with `patches/tools/remap_talents.py`. Points on removed talents were dropped
(listed in `SOURCES.json`). Treat those builds as approximations, and pass your own build with
`--talents`:

```powershell
patchsim run --preset frost-mage --talents "000205103-3-053500000101301251"
```

From launch, use your own character. `patchsim import` turns an addon export (the MythicSim
addon or WoWSims Exporter text that MythicSim.com accepts) into a request. It keeps the
preset's buffs, consumables and rotation and takes your race, talents, gear and professions:

```powershell
patchsim import --addon my-export.json --preset frost-mage --outfile me.request.json
patchsim run --request me.request.json
```

The export format is read loosely (`equipment.items`, `gear.items` or `items`), since it could not
be checked before launch. If an import fails or looks wrong, send the export text to have the
importer fixed.

## Options

```
patchsim run (--preset NAME | --request FILE)
    --overrides FILE   default patches/overrides.json
    --talents STRING   replace the talent string
    --iterations N     default 10000 (about a second)
    --seed N           default 42
    --duration SECONDS fight length
    --top N            actions listed, default 15
    --json FILE        also write both results as JSON; `-` prints only JSON
patchsim spell QUERY [--full] [--json]
patchsim presets [--json]
patchsim import --addon FILE --preset NAME [--outfile FILE]
```

## Keeping up with upstream

Your changes live in `patches/`, `src/bin/patchsim.rs`, `src/overrides.rs` and a few lines in
`src/data/spells.rs` (`find` and `curve`), `src/lib.rs` and `Cargo.toml`. To take upstream's
latest engine:

```powershell
git remote add upstream https://github.com/sage3648/mythicsim-forever-engine.git   # once
git fetch upstream
git merge upstream/main
cargo build --release
```

Once upstream has shipped a patch you overrode, run with that change disabled and compare. If
the baseline already matches, delete the entry.

## How it works

`patchsim run` starts itself twice (`patchsim __one`), once without and once with the overrides,
because row changes are installed for the whole process. Each run prepares the request under
Forever's rules, applies the prepared changes (`forever_engine::overrides::apply_prepared`),
passes it through the same coverage gate as MythicSim, and simulates it.
