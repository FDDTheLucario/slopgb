import struct, subprocess, os, re, shutil, sys
RAW = {'0':[0x7F,0x41,0x41,0x41,0x41,0x41,0x7F],'1':[0x08,0x08,0x08,0x08,0x08,0x08,0x08],
 '2':[0x7F,0x01,0x01,0x7F,0x40,0x40,0x7F],'3':[0x7F,0x01,0x01,0x3F,0x01,0x01,0x7F],
 '4':[0x41,0x41,0x41,0x7F,0x01,0x01,0x01],'5':[0x7F,0x40,0x40,0x7E,0x01,0x01,0x7E],
 '6':[0x7F,0x40,0x40,0x7F,0x41,0x41,0x7F],'7':[0x7F,0x01,0x02,0x04,0x08,0x10,0x10],
 '8':[0x3E,0x41,0x41,0x3E,0x41,0x41,0x3E],'9':[0x7F,0x41,0x41,0x7F,0x01,0x01,0x7F],
 'A':[0x08,0x22,0x41,0x7F,0x41,0x41,0x41],'B':[0x7E,0x41,0x41,0x7E,0x41,0x41,0x7E],
 'C':[0x3E,0x41,0x40,0x40,0x40,0x41,0x3E],'D':[0x7E,0x41,0x41,0x41,0x41,0x41,0x7E],
 'E':[0x7F,0x40,0x40,0x7F,0x40,0x40,0x7F],'F':[0x7F,0x40,0x40,0x7F,0x40,0x40,0x40]}
def ocr(p,n):
    d=open(p,'rb').read(); off=struct.unpack('<I',d[10:14])[0]
    def px(x,y):
        i=off+(y*160+x)*4; return (d[i],d[i+1],d[i+2])
    out=''
    for ti in range(n):
        bg=px(ti*8,0); rows=[]
        for y in range(1,8):
            b=0
            for x in range(8): b=(b<<1)|(1 if px(ti*8+x,y)!=bg else 0)
            rows.append(b)
        ch='?'
        for c,g in RAW.items():
            if rows==g: ch=c; break
        out+=ch
    return out
# Prefer the persistent cache build; fall back to the legacy /tmp path (wiped
# between sessions). Override with SBT=... See build_sameboy_tracers.sh.
def _sbt():
    if os.environ.get('SBT'):
        return os.environ['SBT']
    cache = os.path.expanduser('~/.cache/sbbuild/SameBoy-1.0.2/build/bin/tester/sameboy_tester')
    tmp = '/tmp/sbbuild/SameBoy-1.0.2/build/bin/tester/sameboy_tester'
    return cache if os.path.exists(cache) else tmp
SBT=_sbt()
if not os.path.exists(SBT):
    sys.exit(f"sameboy_tester not found at {SBT} — run build_sameboy_tracers.sh or set SBT=. "
             "Classifying with a missing tester is a vacuous result, not a bar.")
ROOT='/home/soulcatcher/personal_repos/slopgb/test-roms/game-boy-test-roms-v7.0'
rows=[l.strip() for l in open(sys.argv[1]) if l.strip()]
bug=[];floor=[];unk=[]
for rel in rows:
    m=re.search(r'cgb04c_out([0-9A-Fa-f]+)\.gb',rel)
    if not m: unk.append(rel); continue
    want=m.group(1).upper()
    src=os.path.join(ROOT,rel)
    if not os.path.exists(src): unk.append(rel); continue
    shutil.copy(src,'/tmp/s7/cls.gbc')
    subprocess.run([SBT,'--cgb','--length','4','/tmp/s7/cls.gbc'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    if not os.path.exists('/tmp/s7/cls.bmp'): unk.append(rel); continue
    sb=ocr('/tmp/s7/cls.bmp',len(want))
    if '?' in sb: unk.append((rel,sb,want)); continue
    if sb==want: bug.append(rel)
    else: floor.append((rel,sb,want))
print(f"BUG(sb==want, must FIX)={len(bug)}  FLOOR/DIFF(sb!=want, baseline at flip)={len(floor)}  UNK={len(unk)}")
open('/tmp/s7/buglist.txt','w').write('\n'.join(bug))
open('/tmp/s7/floorlist.txt','w').write('\n'.join(f"{r}\tsb={s}\twant={w}" for r,s,w in floor))
