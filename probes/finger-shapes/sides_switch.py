"""Hand switch at the same spot: hand X alone (last 2 s of a one-hand
segment), gap g, hand Y alone (first 3 s of a one-hand segment of the other
hand), Y shifted so its first frame starts where X left. Counts Y frames
labelled with X's side.  python3 sides_switch.py raw S1,S2,S3 '<variant json>'"""
import json, sys, statistics as st
from sides_sim import *
def frames(L, seg):
    a=seg["start_s_cam0"]+seg["margin_s"][0]; b=seg["end_s_cam0"]-seg["margin_s"][1]
    out=[]
    for ln in L:
        if "raw" in ln and a<=ln["t"]<=b:
            cs=hands_of(ln["raw"])
            if len(cs)==1:
                c=cs[0]; fcx=ln["raw"]["face"][0]*KX if "face" in ln["raw"] else None
                out.append((ln["t"],[c["c"][0]*KX,c["c"][1]*KY],c,fcx))
    return out
def main(D, ss, var):
    wrong=[]; 
    for s in ss:
        L=[json.loads(l) for l in open(f"{D}/ba_{s}.jsonl") if l.strip()]
        segs=[x for x in json.load(open(DATA+SESS[s]+"/segments.json")) if x["block"]=="A" and not x["bad"]]
        one={"L":[],"R":[]}
        for seg in segs:
            l,r=seg["labels"].get("hand_L","absent"),seg["labels"].get("hand_R","absent")
            if (l=="absent")!=(r=="absent"):
                f=frames(L,seg)
                if len(f)>30: one["R" if l=="absent" else "L"].append(f)
        for x,y in (("L","R"),("R","L")):
            for fx in one[x]:
                for fy in one[y]:
                    for gap in (0.1,0.3):
                        tr=Tracker(cap=var.get("cap",40.0),gap=var.get("gap"))
                        A=[f for f in fx if f[0]>=fx[-1][0]-2.0]; B=[f for f in fy if f[0]<=fy[0][0]+3.0]
                        dx=[A[-1][1][0]-B[0][1][0],A[-1][1][1]-B[0][1][1]]
                        t0=A[-1][0]+gap-B[0][0]
                        for t,p,c,fcx in A: tr.assign([(p,vote(c))],fcx,t)
                        n=0
                        for t,p,c,fcx in B:
                            side=tr.assign([([p[0]+dx[0],p[1]+dx[1]],vote(c))],fcx,t+t0)[0]
                            n+= side!=y
                        wrong.append(n)
    w=sorted(wrong)
    print(var,"switches",len(w),"wrong frames med",st.median(w),"p90",w[9*len(w)//10],"max",w[-1],"share >3 frames %.3f"%(sum(x>3 for x in w)/len(w)))
if __name__=="__main__":
    main(sys.argv[1],sys.argv[2].split(","),json.loads(sys.argv[3]) if len(sys.argv)>3 else {})
