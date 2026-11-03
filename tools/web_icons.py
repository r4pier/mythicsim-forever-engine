#!/usr/bin/env python3
"""Write src/web/icons.json, the icon names the patchsim web page shows for talents, buffs,
debuffs and consumables.

Icon names come from Wowhead's tooltip API (nether.wowhead.com), tried for each spell ID of a
talent or buff name in data/spells.jsonl across the classic, tbc, wotlk and retail databases.
Forever-only spells (IDs Wowhead does not know) fall back to OVERRIDES, which name an older spell
with the same icon. The page draws the images from wow.zamimg.com and looks up gear icons live.

Rerun after an upstream sync that adds talents:  python3 tools/web_icons.py
Needs network access. Uses only Python's standard library.
"""
import collections
import concurrent.futures
import json
import re
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FLAVORS = ["classic", "tbc", "wotlk", ""]
# Names whose Forever spells Wowhead lacks, or proto fields that are not spell names: an icon
# name directly.
OVERRIDES = {
    "arcane blast": "spell_arcane_blast",
    "missile barrage": "ability_mage_missilebarrage",
    "arcane wrath": "spell_arcane_arcane04",
    "heating up": "ability_mage_hotstreak",
    "incineration": "spell_fire_burnout",
    "fingers of frost": "ability_mage_wintersgrasp",
    "ice lance": "spell_frost_frostblast",
    "mana tide totems": "spell_frost_summonwaterelemental",
    "retribution aura spell power": "spell_holy_auraoflight",
    "atiesh mage": "inv_staff_medivh",
    "atiesh warlock": "inv_staff_medivh",
    "atiesh druid": "inv_staff_medivh",
    "atiesh priest": "inv_staff_medivh",
    "totem twisting": "spell_nature_windfury",
    "greater blessing of salvation": "spell_holy_greaterblessingofsalvation",
    "innervates": "spell_nature_lightning",
    "power infusions": "spell_holy_powerinfusion",
    "hunters mark": "ability_hunter_snipershot",
    "curse of elements": "spell_shadow_chilltouch",
    "no hit suppression": "ability_marksmanship",
    "holy shock": "spell_holy_searinglight",
    "turn evil": "spell_holy_turnundead",
    "accelerant": "spell_fire_flamebolt",
    "improved mind flay": "spell_shadow_siphonmana",
    "improved felhunter": "spell_shadow_summonfelhunter",
    "improved stormstrike": "spell_holy_sealofmight",
    "improved distract": "ability_rogue_distract",
    "summon hawk": "ability_hunter_pet_bat",
    "sniper shot": "ability_hunter_snipershot",
    "venom": "ability_rogue_dualweild",
}
# A tree's icon, for the Forever talents Wowhead has no spell for.
TREES = {
    "druid": ["spell_nature_starfall", "ability_racial_bearform", "spell_nature_healingtouch"],
    "hunter": ["ability_hunter_beasttaming", "ability_marksmanship", "ability_hunter_swiftstrike"],
    "mage": ["spell_holy_magicalsentry", "spell_fire_firebolt02", "spell_frost_frostbolt02"],
    "paladin": ["spell_holy_holybolt", "spell_holy_devotionaura", "spell_holy_auraoflight"],
    "priest": ["spell_holy_wordfortitude", "spell_holy_guardianspirit", "spell_shadow_shadowwordpain"],
    "rogue": ["ability_rogue_eviscerate", "ability_backstab", "ability_stealth"],
    "shaman": ["spell_nature_lightning", "spell_nature_lightningshield", "spell_nature_magicimmunity"],
    "warlock": ["spell_shadow_deathcoil", "spell_shadow_metamorphosis", "spell_shadow_rainoffire"],
    "warrior": ["ability_warrior_savageblow", "ability_warrior_innerrage", "ability_warrior_defensivestance"],
}


def norm(name):
    """A name without case or punctuation: the talent layouts spell some names differently."""
    return re.sub(r"[^a-z0-9]", "", name.lower())


def fetch(kind, entity):
    for flavor in FLAVORS:
        url = f"https://nether.wowhead.com/{flavor + '/' if flavor else ''}tooltip/{kind}/{entity}"
        try:
            req = urllib.request.Request(url, headers={"User-Agent": "patchsim-web-icons"})
            with urllib.request.urlopen(req, timeout=20) as res:
                data = json.load(res)
        except Exception:
            continue
        if data.get("icon"):
            return data["icon"]
    return None


def main():
    by_name = collections.defaultdict(list)
    for line in (ROOT / "data/spells.jsonl").read_text().splitlines():
        row = json.loads(line)
        by_name[norm(row["Name"])].append(row["ID"])
    talents = json.loads((ROOT / "src/web/talents.json").read_text())
    schema = json.loads((ROOT / "data/proto-schema.json").read_text())["messages"]

    wanted = {}  # display name (lowercase) -> spell ids
    talent_keys = {}
    for cls, trees in talents.items():
        for tree in trees:
            for talent in tree["talents"]:
                key = talent["name"].lower()
                talent_keys[(cls, talent["field"])] = key
                wanted[key] = sorted(by_name.get(norm(key), []), key=lambda i: (i > 1_000_000, i))
    buff_keys = {}
    for message in ["proto.RaidBuffs", "proto.PartyBuffs", "proto.IndividualBuffs", "proto.Debuffs"]:
        for field in schema[message]["fields"]:
            key = re.sub(r"([a-z])([A-Z])", r"\1 \2", field["json_name"]).lower()
            buff_keys[field["json_name"]] = key
            # The highest rank first: the buff a raid brings.
            wanted[key] = sorted(by_name.get(norm(key), []), key=lambda i: (i > 1_000_000, -i))

    def resolve(key):
        if key in OVERRIDES and key != "no hit suppression":
            return key, OVERRIDES[key]
        for spell in wanted[key][:4]:
            icon = fetch("spell", spell)
            if icon:
                return key, icon
        return key, None

    with concurrent.futures.ThreadPoolExecutor(12) as pool:
        icons = dict(pool.map(resolve, wanted))

    consumables = {}
    rows = [json.loads(l) for l in (ROOT / "data/consumables.jsonl").read_text().splitlines() if l.strip()]
    with concurrent.futures.ThreadPoolExecutor(12) as pool:
        for row, icon in zip(rows, pool.map(lambda r: fetch("item", r["id"]), rows)):
            if icon:
                consumables[str(row["id"])] = icon

    out = {
        "talents": {cls: {} for cls in talents},
        "buffs": {},
        "consumables": consumables,
        "extras": {key: OVERRIDES[key] for key in ["no hit suppression"]},
        "trees": TREES,
    }
    for (cls, field), key in talent_keys.items():
        if icons.get(key):
            out["talents"][cls][field] = icons[key]
    for field, key in buff_keys.items():
        if icons.get(key):
            out["buffs"][field] = icons[key]
    missing = sorted(key for key, icon in icons.items() if not icon)
    (ROOT / "src/web/icons.json").write_text(json.dumps(out, indent=1, sort_keys=True) + "\n")
    print(f"{sum(len(v) for v in out['talents'].values())} talents, {len(out['buffs'])} buffs, "
          f"{len(consumables)}/{len(rows)} consumables; no icon: {missing}")


if __name__ == "__main__":
    main()
