"""Replays SideTracker (geom.rs) on raw bench candidates and scores sides.
  python3 sides_sim.py <raw dir> S1,S2,S3 [variant]
Frame px: 1280x720, seg 384x216."""
import json, sys, math
DATA="/Users/no8/Desktop/life/Lifemaxxing/synthexis/data/recordings/"
SESS={"S1":"2026-10-27-S1","S2":"2026-10-29-S2","S3":"2026-11-01-S3","S4":"2026-11-01-S4"}
FW,FH=1280.0,720.0; KX,KY=FW/384,FH/216
EXPIRE=0.5; MATCH=0.25

def lean(kp,c):
    d=lambda a,b: math.hypot(a[0]-b[0],a[1]-b[1])
    palm=max(d(kp[0],kp[9]),1.2*d(kp[5],kp[17]),1.0)
    return max(-1,min(1,(kp[0][0]-c[0])/palm))

def vote(cand, use_lean=True, use_ch=True):
    kp=cand["kp"]
    if not kp: return 0.0
    kp=[kp[i:i+2] for i in range(0,42,2)]
    c=[cand["c"][0]*KX,cand["c"][1]*KY]
    v=lean(kp,c) if use_lean else 0.0
    if use_ch: v+=max(-1,min(1,-cand["ch"]/2))
    return v

class Tracker:
    def __init__(s,cap=40.0,prior=0.3,gap=None): s.tr=[]; s.cap=cap; s.prior=prior; s.gap=gap
    def assign(s,hands,face_cx,t):
        s.tr=[x for x in s.tr if t-x["t"]<EXPIRE]
        pairs=sorted((math.hypot(h[0][0]-x["pos"][0],h[0][1]-x["pos"][1]),i,j) for i,h in enumerate(hands) for j,x in enumerate(s.tr))
        tof=[None]*len(hands); taken=[False]*len(s.tr)
        for d,i,j in pairs:
            ok = d<MATCH*FW and tof[i] is None and not taken[j]
            if ok and s.gap is not None and t-s.tr[j]["t"]>s.gap: ok=False
            if ok: tof[i]=j; taken[j]=True
        ref=face_cx if face_cx is not None else FW/2
        idx=[]
        for h,m in zip(hands,tof):
            if m is None:
                p=s.prior*max(-1,min(1,(h[0][0]-ref)/(0.25*FW)))
                s.tr.append({"pos":h[0],"t":t,"lean":p}); m=len(s.tr)-1
            idx.append(m)
        for h,j in zip(hands,idx):
            x=s.tr[j]; x["lean"]=max(-s.cap,min(s.cap,x["lean"]+h[1])); x["pos"]=h[0]; x["t"]=t
        if len(hands)==2: sides=["R","L"] if hands[0][0][0]<hands[1][0][0] else ["L","R"]
        elif len(hands)==1: sides=["L" if s.tr[idx[0]]["lean"]>0 else "R"]
        else: sides=[]
        extra=max(0,len(s.tr)-2); s.tr=s.tr[extra:]
        return sides

def hands_of(line):
    """hand_blobs: area>=60, at most two, largest first; dump keeps area>=20 top4 in component order."""
    cs=[c for c in line.get("cands",[]) if c["a"]>=60]
    cs.sort(key=lambda c:-c["a"])
    cs=cs[:2]
    if len(cs)==2 and cs[1]["a"]<0.1*cs[0]["a"]: cs=cs[:1]
    return cs

def run(D, sessions, variant):
    cap=variant.get("cap",40.0); gap=variant.get("gap")
    tot=[0,0]; agree=[0,0]; per=[]
    for s in sessions:
        L=[json.loads(l) for l in open(f"{D}/ba_{s}.jsonl") if l.strip()]
        segs=[x for x in json.load(open(DATA+SESS[s]+"/segments.json")) if x["block"]=="A" and not x["bad"]]
        tr=Tracker(cap=cap,gap=gap)
        out={}
        for ln in L:
            if "raw" not in ln: continue
            cs=hands_of(ln["raw"])
            fcx=ln["raw"]["face"][0]*KX if "face" in ln["raw"] else None
            hs=[([c["c"][0]*KX,c["c"][1]*KY],vote(c,variant.get("lean",True),variant.get("ch",True))) for c in cs]
            out[round(ln["t"],3)]=(tr.assign(hs,fcx,ln["t"]), ln)
        for seg in segs:
            lab=seg["labels"]; l,r=lab.get("hand_L","absent"),lab.get("hand_R","absent")
            if (l=="absent")==(r=="absent"): continue
            want="R" if l=="absent" else "L"
            a=seg["start_s_cam0"]+seg["margin_s"][0]; b=seg["end_s_cam0"]-seg["margin_s"][1]
            n=bad=0
            for t,(sides,ln) in out.items():
                if a<=t<=b and len(sides)==1:
                    n+=1; bad+= sides[0]!=want
                    eng=[k[5] for k in ("hand_L","hand_R") if k in ln]
                    if len(eng)==1: agree[0]+= eng[0]==sides[0]; agree[1]+=1
            tot[0]+=bad; tot[1]+=n
            if bad: per.append((s,seg["id"],bad,n))
    print(variant, "wrong", tot[0],"/",tot[1], f"{tot[0]/max(1,tot[1]):.4f}", "sim==engine", f"{agree[0]}/{agree[1]}")
    for p in per: print("  ",p)

if __name__=="__main__":
    D=sys.argv[1]; ss=sys.argv[2].split(",")
    v=json.loads(sys.argv[3]) if len(sys.argv)>3 else {}
    run(D,ss,v)
