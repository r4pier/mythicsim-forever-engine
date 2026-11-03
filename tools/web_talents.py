#!/usr/bin/env python3
"""Write src/web/talents.json, the talent trees the patchsim web page draws.

Classes whose trees Forever reorders keep their layout in Rust (each tree's talents in the
client's order with their ranks): mage, paladin, hunter and druid. Their layouts are read from
that source. The other classes keep the reference order of their proto.<Class>Talents fields,
split by the class's TALENT_TREE_SIZES, and their ranks come from the talent curves in
data/spell-extras.json where a talent spell of that name has one (5 otherwise; the engine
refuses a string above a talent's ranks either way).

Rerun after an upstream sync that changes talents:  python3 tools/web_talents.py
Uses only Python's standard library.
"""
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CLASSES = ["druid", "hunter", "mage", "paladin", "priest", "rogue", "shaman", "warlock", "warrior"]
LAYOUTS = {
    "mage": ("src/classes/mage/prepare.rs", "FOREVER_TREES"),
    "paladin": ("src/classes/paladin/prepare/hotfix_113554.rs", "TREES"),
    "hunter": ("src/classes/hunter/prepare.rs", "FOREVER_TREES"),
    "druid": ("src/classes/druid/prepare.rs", "FOREVER_TREES"),
}
TREE_NAMES = {
    "druid": ["Balance", "Feral Combat", "Restoration"],
    "hunter": ["Beast Mastery", "Marksmanship", "Survival"],
    "mage": ["Arcane", "Fire", "Frost"],
    "paladin": ["Holy", "Protection", "Retribution"],
    "priest": ["Discipline", "Holy", "Shadow"],
    "rogue": ["Assassination", "Combat", "Subtlety"],
    "shaman": ["Elemental", "Enhancement", "Restoration"],
    "warlock": ["Affliction", "Demonology", "Destruction"],
    "warrior": ["Arms", "Fury", "Protection"],
}
TALENT = re.compile(r'(?:new_)?talent\(\s*"([a-z0-9_]+)",\s*"([^"]+)",\s*(\d+),?\s*\)', re.S)


def layout(path, trees_const):
    source = (ROOT / path).read_text()
    names = re.search(r"const " + trees_const + r":[^=]*=\s*\[([^\]]*)\]", source).group(1)
    trees = []
    for const in [name.strip() for name in names.split(",") if name.strip()]:
        body = re.search(r"const " + const + r":[^=]*=\s*&\[(.*?)\n\];", source, re.S).group(1)
        trees.append([{"field": f, "name": n, "max": int(r)} for f, n, r in TALENT.findall(body)])
    return trees


def curve_ranks():
    curves = json.loads((ROOT / "data/spell-extras.json").read_text())["curves"]
    by_name = {}
    for line in (ROOT / "data/spells.jsonl").read_text().splitlines():
        row = json.loads(line)
        curve = curves.get(str(row["ID"]))
        if curve:
            by_name.setdefault(row["Name"].lower(), len(curve[0]))
    return by_name


def title(field):
    small = {"of", "the", "and"}
    words = field.split("_")
    return " ".join(w if (i and w in small) else w.capitalize() for i, w in enumerate(words))


def main():
    schema = json.loads((ROOT / "data/proto-schema.json").read_text())["messages"]
    ranks = curve_ranks()
    out = {}
    for cls in CLASSES:
        if cls in LAYOUTS:
            trees = layout(*LAYOUTS[cls])
        else:
            source = (ROOT / f"src/classes/{cls}/prepare.rs").read_text()
            sizes = [int(n) for n in re.search(
                r"TALENT_TREE_SIZES: \[usize; 3\] = \[(\d+), (\d+), (\d+)\]", source).groups()]
            fields = sorted(schema[f"proto.{cls.capitalize()}Talents"]["fields"],
                            key=lambda field: field["number"])
            trees, start = [], 0
            for size in sizes:
                tree = []
                for field in fields[start:start + size]:
                    name = title(field["name"])
                    max_ranks = 1 if field["kind"] == "bool" else ranks.get(name.lower(), 5)
                    tree.append({"field": field["name"], "name": name, "max": max_ranks})
                trees.append(tree)
                start += size
        out[cls] = [{"name": TREE_NAMES[cls][i], "talents": tree} for i, tree in enumerate(trees)]
    target = ROOT / "src/web/talents.json"
    target.write_text(json.dumps(out, indent=1) + "\n")
    print(target, {cls: [len(t["talents"]) for t in trees] for cls, trees in out.items()})


if __name__ == "__main__":
    main()
