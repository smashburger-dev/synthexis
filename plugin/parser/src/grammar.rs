//! Port of frame_parser.Grammar: masks closed tokens and pointer positions
//! so every decoded frame is well formed. Combined ids: `< fv` closed
//! token, `fv + p` pointer to encoder position p.

pub const PTR_KINDS: [&str; 7] = ["TRACK", "PLUGIN", "PNAME", "NOTE", "TIME", "CHORD", "VALUE"];
const SPECIAL: [&str; 4] = ["<pad>", "<bos>", "<eos>", "<ptr>"];

pub struct Grammar {
    vocab: Vec<String>,
    intent: Vec<bool>,
    ptrk: Vec<bool>,
    tparam: Vec<bool>,
    objshape: Vec<bool>,
    mode: Vec<bool>,
    scope: Vec<bool>,
    open: Vec<bool>,
    close: Vec<bool>,
    eos: Vec<bool>,
    base: Vec<bool>,
    ft: Vec<bool>,
    max_steps: usize,
    max_span: usize,
}

impl Grammar {
    pub fn new(vocab: &[String], max_steps: usize, max_span: usize) -> Self {
        let m = |f: &dyn Fn(&str) -> bool| vocab.iter().map(|t| f(t)).collect::<Vec<bool>>();
        let intent = m(&|t| t.starts_with("I:"));
        let objshape = m(&|t| t.starts_with("object:") || t.starts_with("shape:"));
        let mode = m(&|t| t.starts_with("mode:"));
        let scope = m(&|t| t.starts_with("scope:"));
        let open = m(&|t| t == "<route>");
        let close = m(&|t| t == "</route>");
        let eos = m(&|t| t == "<eos>");
        let special = m(&|t| SPECIAL.contains(&t));
        let base = (0..vocab.len())
            .map(|i| {
                !(intent[i]
                    || objshape[i]
                    || mode[i]
                    || scope[i]
                    || open[i]
                    || close[i]
                    || eos[i]
                    || special[i])
            })
            .collect();
        Self {
            ptrk: m(&|t| PTR_KINDS.contains(&t)),
            tparam: m(&|t| t.starts_with("T:param:")),
            ft: m(&|t| t == "FROM" || t == "TO"),
            vocab: vocab.to_vec(),
            intent,
            objshape,
            mode,
            scope,
            open,
            close,
            eos,
            base,
            max_steps,
            max_span,
        }
    }

    fn closed(&self, id: u32) -> Option<usize> {
        let i = id as usize;
        (i < self.vocab.len()).then_some(i)
    }

    fn is_ptr_kind(&self, id: u32) -> bool {
        self.closed(id).is_some_and(|i| self.ptrk[i])
    }

    /// seq: combined ids so far; n: encoder length incl. BOS.
    pub fn allowed(&self, seq: &[u32], n: usize) -> Vec<bool> {
        let fv = self.vocab.len();
        let mut out = vec![false; fv + n];
        let set = |out: &mut Vec<bool>, mask: &[bool]| out[..fv].copy_from_slice(mask);
        let Some(&last) = seq.last() else {
            set(&mut out, &self.intent);
            return out;
        };
        if self.is_ptr_kind(last) {
            out[fv + 1..fv + n].iter_mut().for_each(|a| *a = true);
            return out;
        }
        if self.closed(last).is_none() && seq.len() >= 2 && self.is_ptr_kind(seq[seq.len() - 2]) {
            let p = last as usize - fv;
            let end = n.min(p + self.max_span + 1);
            if fv + p < fv + end {
                out[fv + p..fv + end].iter_mut().for_each(|a| *a = true);
            }
            return out;
        }
        if let Some(i) = self.closed(last) {
            if self.ft[i] {
                set(&mut out, &self.tparam);
                return out;
            }
        }
        let count = |mask: &[bool]| {
            seq.iter()
                .filter(|&&t| self.closed(t).is_some_and(|i| mask[i]))
                .count()
        };
        let in_route = count(&self.open) > count(&self.close);
        if seq.len() + 2 >= self.max_steps {
            // force the frame to finish
            set(&mut out, if in_route { &self.close } else { &self.eos });
            return out;
        }
        let mut a = self.base.clone();
        let lt = self.closed(last).map_or("", |i| self.vocab[i].as_str());
        let or = |a: &mut Vec<bool>, mask: &[bool]| {
            a.iter_mut().zip(mask).for_each(|(x, &m)| *x |= m);
        };
        if ["S:", "G:", "side:", "object:", "shape:"]
            .iter()
            .any(|p| lt.starts_with(p))
        {
            or(&mut a, &self.objshape);
        }
        if lt.starts_with("T:") || lt.starts_with("mode:") {
            or(&mut a, &self.mode);
        }
        if in_route {
            if lt != "<route>" {
                or(&mut a, &self.close);
            }
        } else {
            or(&mut a, &self.open);
            or(&mut a, &self.eos);
            or(&mut a, &self.scope);
        }
        set(&mut out, &a);
        out
    }
}
