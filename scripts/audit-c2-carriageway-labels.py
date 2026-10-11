#!/usr/bin/env python3
"""Audit the C2 carriageway labels against official ramp directions.

The derived split in graph-builder decides inner/outer from ramp reachability,
which fails closed near JCTs. This audit uses a different signal chain:

1. anchors: for every verified_bound C2 ramp, the ramp's own OSM way ends (entry)
   or starts (exit) at the junction with the mainline. The membership edge
   nearest that point whose heading matches the ramp's travel direction is on
   that ramp's carriageway -- the ramp directions come from the official route
   map through data/ramp-inventory.json, not from the derived split.
2. propagation: an edge is classified by exact geometry only -- collinear
   continuation within 300 m (same carriageway) or an antiparallel pair within
   60 m (the other carriageway). No ramp-reachability filter is involved.

The residual is printed per member way, so the places where the two carriageways
are stacked or co-located (a JCT interior, 飛鳥山トンネル) show up as ways the
audit cannot decide instead of being silently guessed.

    python3 scripts/audit-c2-carriageway-labels.py
"""
import json, math
from collections import Counter, defaultdict
graph=json.load(open('fixtures/generated/graph.json'))
nodes={n['id']:(n['lat']*111320.0, n['lon']*111320.0*math.cos(math.radians(n['lat']))) for n in graph['nodes']}
edges={e['id']:e for e in graph['edges']}
membership={}
for m in graph['routeMemberships']:
    if not m['membershipId'].startswith('route:C2'): continue
    for s in m['segments']:
        if s['sourceKind']!='relationMainline': continue
        for eid in s['orderedEdgeIds']: membership[eid]=m['direction']
inv=json.load(open('data/ramp-inventory.json'))
bind={b['rampId']:b for b in json.load(open('data/osm-ramp-bindings.json'))['bindings']}
def way_of(eid): return int(eid.split(':')[1][1:])
def seg(eid): return int(eid.split(':')[2])
def geo(e):
    a,b=nodes[e['from']],nodes[e['to']]
    mid=((a[0]+b[0])/2,(a[1]+b[1])/2); dx,dy=b[0]-a[0],b[1]-a[1]; n=math.hypot(dx,dy) or 1.0
    return mid,(dx/n,dy/n)
way_edges=defaultdict(list)
for e in graph['edges']:
    if e['kind'] in ('entry','exit','shutoko'): way_edges[way_of(e['id'])].append(e)
for v in way_edges.values(): v.sort(key=lambda e: seg(e['id']))
anchors={}; missing=[]
for r in inv['ramps']:
    rid=r['rampId']
    if not rid.startswith('ramp:c2-') or r.get('supportState')!='verified_bound': continue
    b=bind.get(rid); eids=way_edges.get(b['osmWayId'],[]) if b else []
    if not eids: missing.append((rid,'no edges')); continue
    is_exit=r['kind'] in ('general_exit','boundary_out')
    junction=eids[0]['from'] if is_exit else eids[-1]['to']
    p=nodes[junction]; _,rh=geo(eids[0] if is_exit else eids[-1])
    best=None
    for eid in membership:
        mid,h=geo(edges[eid]); dist=math.hypot(mid[0]-p[0],mid[1]-p[1])
        if dist>150: continue
        cos=h[0]*rh[0]+h[1]*rh[1]
        if cos<0.7: continue
        if best is None or dist<best[0]: best=(dist,eid)
    if best is None: missing.append((rid,'no aligned edge <=150 m')); continue
    anchors[best[1]]=r['direction']
print(f'anchors: {len(anchors)}/25; missing: {[m[0] for m in missing]}')
print('anchor conflicts with derived split:', [(e,membership[e],d) for e,d in anchors.items() if membership[e]!=d])
# grid over ALL membership edges
grid=defaultdict(list); cell=0.003
def cell_of(p): return (int(p[0]//(cell*111320)), int(p[1]//(cell*111320)))
geo_cache={eid:geo(edges[eid]) for eid in membership}
for eid in membership: grid[cell_of(geo_cache[eid][0])].append(eid)
classified=dict(anchors)
def flip(d): return 'outer' if d=='inner' else 'inner'
for _ in range(200):
    added=[]
    for eid,d in list(classified.items()):
        mid,h=geo_cache[eid]; cx,cy=cell_of(mid)
        for dx in (-2,-1,0,1,2):
            for dy in (-2,-1,0,1,2):
                for other in grid.get((cx+dx,cy+dy),()):
                    if other in classified: continue
                    omid,oh=geo_cache[other]; dist=math.hypot(omid[0]-mid[0],omid[1]-mid[1])
                    cos=h[0]*oh[0]+h[1]*oh[1]
                    if cos>=0.99 and dist<=300: added.append((other,d))
                    elif cos<=-0.9 and dist<=60: added.append((other,flip(d)))
    if not added: break
    new=0
    for eid,d in added:
        if eid not in classified:
            classified[eid]=d; new+=1
    if new==0: break
print(f'classified membership edges: {len(classified)}/{len(membership)}')
osm=json.load(open('fixtures/osm/shutoko-all.json'))
rel=next(e for e in osm['elements'] if e['type']=='relation' and e['id']==4256077)
members=[m['ref'] for m in rel['members'] if m['type']=='way']
role_of={r['wayId']:r['role'] for r in json.load(open('data/osm-relation-repairs.json'))['repairs'] if r['kind']=='setMemberRole'}
counts=Counter(); residual=[]; partial=[]
for w in members:
    eids=[e for e in membership if way_of(e)==w]
    votes=Counter(classified[e] for e in eids if e in classified)
    if not votes: counts['unclassified']+=1; continue
    win,c=votes.most_common(1)[0]; role=role_of.get(w)
    rd=('inner' if role.startswith('Inner') else 'outer') if role else None
    if rd==win and c==len(eids): counts['confirmed']+=1
    elif rd==win: counts['confirmed(partial)']+=1; partial.append((w,win,dict(votes),len(eids)))
    elif role is None: counts['not in file']+=1
    else: counts['disagrees']+=1; residual.append((w,rd,win,dict(votes),len(eids)))
print('per-way summary:', dict(counts))
print('partial:', partial)
for w,rd,win,votes,total in residual:
    ws=next(e for e in osm['elements'] if e['type']=='way' and e['id']==w)
    nn=next(x for x in graph['nodes'] if x['id']==f'n:{ws["nodes"][0]}')
    print(f'  disagree: {w:<12} file={rd:<6} anchor-based={win:<6} votes={votes}/{total} at {nn["lat"]:.4f},{nn["lon"]:.4f} ({ws.get("tags",{}).get("highway")})')
