"""Re-map a pre-hotfix talent string (Go reference order) onto Forever's current tree order by talent field.

Usage: python3 patches/tools/remap_talents.py REPO_ROOT ClassMage|ClassDruid|ClassPaladin TALENTS
Prints the new string and the points it could not place (talents removed or above their new ranks).
"""
import json, re, sys, os
root = sys.argv[1]
schema = json.load(open(root + '/data/proto-schema.json'))['messages']

def rust_trees(path, names):
    src = open(root + '/' + path).read()
    trees = []
    for n in names:
        m = re.search(r'const ' + n + r': &\[\w+\] = &\[(.*?)\n\];', src, re.S)
        trees.append([(f, int(r)) for f, r in re.findall(r'talent\(\s*"(\w+)",\s*"[^"]*",\s*(\d+)\s*,?\s*\)', m.group(1))])
    return trees

LAYOUTS = {
  'ClassMage': ('proto.MageTalents', [19,17,18], rust_trees('src/classes/mage/prepare.rs', ['FOREVER_ARCANE','FOREVER_FIRE','FOREVER_FROST'])),
  'ClassDruid': ('proto.DruidTalents', [16,20,16], rust_trees('src/classes/druid/prepare.rs', ['FOREVER_BALANCE','FOREVER_FERAL_COMBAT','FOREVER_RESTORATION'])),
  'ClassPaladin': ('proto.PaladinTalents', [17,16,17], rust_trees('src/classes/paladin/prepare/hotfix_113554.rs', ['HOLY','PROTECTION','RETRIBUTION'])),
}

def migrate(cls, s):
    proto, sizes, new = LAYOUTS[cls]
    fields = sorted(schema[proto]['fields'], key=lambda f: f['number'])
    pts = {}; off = 0
    for t, digits in enumerate(s.split('-')):
        for i, d in enumerate(digits):
            if i >= sizes[t]: raise ValueError(f'tree {t+1} longer than the Go order')
            if d.isdigit() and int(d): pts[fields[off+i]['name']] = int(d)
        off += sizes[t]
    out = []; lost = dict(pts)
    for tree in new:
        ds = ''
        for f, r in tree:
            v = min(pts.get(f, 0), r); ds += str(v)
            if f in lost:
                lost[f] -= v
                if lost[f] <= 0: del lost[f]
        out.append(ds.rstrip('0'))
    while out and out[-1] == '': out.pop()
    return '-'.join(out), lost

if __name__ == '__main__':
    cls, s = sys.argv[2], sys.argv[3]
    print(migrate(cls, s))
