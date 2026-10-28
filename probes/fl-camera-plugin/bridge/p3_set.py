import sys, time, json
sys.path.insert(0, '.')
from flb import call
from h2_run import find_channel, find_params, set_enum, sample, must
ch, info = find_channel("synthexis probe")
p = find_params(ch)
must(call("pattern", n=40)); must(call("setpos", pos=0)); set_enum(p, ch, "drive", "off")
ctrl = sample(ch, p["signal"], 4)
must(call("transport", op="record")); must(call("transport", op="start"))
t0 = time.time(); n = 0
while time.time() - t0 < 10:
    ph = ((time.time() - t0) % 4) / 2
    v = ph if ph < 1 else 2 - ph
    must(call("set", ch=ch, idx=p["signal"], value=v)); n += 1
    time.sleep(0.05)
must(call("transport", op="stop")); must(call("transport", op="record"))
must(call("set", ch=ch, idx=p["signal"], value=0.5))
must(call("setpos", pos=0))
j = sample(ch, p["signal"], 8)
res = {"sets": n, "control_range": max(ctrl)-min(ctrl), "judge_range": max(j)-min(j), "distinct": len({round(x,4) for x in j}), "judge_head": [round(x,3) for x in j[:40:4]]}
print(json.dumps(res))
json.dump(res, open('runs/p3_set_pattern40.json','w'))
