import json,sys,statistics as st
fr=[json.loads(l) for l in open(sys.argv[1])]; segs=[x for x in json.load(open(sys.argv[2])) if x["block"]=="C" and not x["bad"]]
T={"C_neutral":(1,1),"C_eyes_closed":(0,0),"C_wink_L":(0,1),"C_wink_R":(1,0)}
out=[];err=[]
for sg in segs:
    if sg["id"] not in ("C_neutral","C_eyes_closed","C_wink_L","C_wink_R","C_smile","C_mouth_wide"): continue
    m0,m1=sg.get("margin_s",[0,0]); a,b=sg["start_s_cam0"]+m0,sg["end_s_cam0"]-m1
    w=[f["face"] for f in fr if a<=f["t"]<=b and "face" in f]
    if sg["id"] in T: err+=[abs(x[2]-T[sg["id"]][0]) for x in w]+[abs(x[3]-T[sg["id"]][1]) for x in w]
    out.append(f'{sg["id"][2:]}=eye({st.mean(x[2] for x in w):.2f},{st.mean(x[3] for x in w):.2f}) sm{st.mean(x[1] for x in w):.2f}')
print(sys.argv[3], "eye err", round(st.mean(err),3), " ".join(out))
