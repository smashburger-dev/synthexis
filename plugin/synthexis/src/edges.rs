//! Routing edges shared by the chat field (sentence.rs) and the hands'
//! node view (fingers.rs): what a route connects, whether it still waits
//! for Keep, and the batch it came in (docs/design/ui-redesign.md, Unit 1
//! contract, R19).

use crate::fl;
use crate::i18n::{fill, Text};

/// What a route drives.
#[derive(Clone, Debug, PartialEq)]
pub enum NodeTarget {
    /// A plugin parameter: the plugin's label (`Names::label`: its place
    /// added when another instance has its name), the parameter name and
    /// the instance (fl::Key id), so two instances of one plugin are two
    /// targets.
    Param { plugin: String, name: String, inst: String },
    /// One of the 32 Signal macros (0-based).
    Macro(usize),
    /// A MIDI part, by its store id and name.
    Part { id: u64, name: String },
}

impl NodeTarget {
    /// The same target whatever its label says: a parameter by instance
    /// and name.
    pub fn same(&self, o: &NodeTarget) -> bool {
        match (self, o) {
            (NodeTarget::Param { name, inst, .. }, NodeTarget::Param { name: n, inst: i, .. }) => name == n && inst == i,
            _ => self == o,
        }
    }
}

/// Which plugin names stand for two or more instances (in FL's inventory
/// or among the routes), and the inventory that knows where a generator's
/// channel goes.
#[derive(Clone, Debug, Default)]
pub struct Names {
    twins: Vec<String>,
    inv: Vec<fl::Target>,
}

impl Names {
    /// The twins among `keys` and the inventory `inv`.
    pub fn new<'a>(keys: impl IntoIterator<Item = &'a fl::Key>, inv: &[fl::Target]) -> Names {
        let mut all: Vec<&fl::Key> = inv.iter().map(|x| &x.key).collect();
        let keys: Vec<&fl::Key> = keys.into_iter().collect();
        for k in keys {
            if !all.contains(&k) {
                all.push(k);
            }
        }
        let mut twins: Vec<String> = Vec::new();
        for (i, k) in all.iter().enumerate() {
            if !twins.contains(&k.plugin) && all.iter().skip(i + 1).any(|o| o.plugin == k.plugin) {
                twins.push(k.plugin.clone());
            }
        }
        Names { twins, inv: inv.to_vec() }
    }

    /// `new` over FL's inventory as the bridge has it now.
    pub fn now<'a>(keys: impl IntoIterator<Item = &'a fl::Key>) -> Names {
        let inv = fl::bridge().view().targets.clone();
        Names::new(keys, &inv)
    }

    pub fn is_twin(&self, plugin: &str) -> bool {
        self.twins.iter().any(|p| p == plugin)
    }

    /// "Serum 2", or with a twin its place: "Serum 2 (Track 3)". An
    /// effect's place is its mixer track, a generator's the track its
    /// channel feeds, else the channel: "Serum 2 (Ch 4)".
    pub fn label(&self, key: &fl::Key, t: &Text) -> String {
        if !self.is_twin(&key.plugin) {
            return key.plugin.clone();
        }
        let track = match key.at {
            fl::At::Fx { track, .. } => Some(track),
            fl::At::Gen { .. } => self.inv.iter().find(|x| x.key == *key).map(|x| x.track).filter(|t| *t > 0),
        };
        let place = match (track, key.at) {
            (Some(0), _) => "Master".to_string(),
            (Some(n), _) => fill(t.place_track, &[&n]),
            (None, fl::At::Gen { ch }) => fill(t.place_ch, &[&(ch + 1)]),
            (None, fl::At::Fx { .. }) => String::new(),
        };
        format!("{} ({place})", key.plugin)
    }
}

/// One edge: a signal id (e.g. `cam0.hand.left.spread`) to its target.
#[derive(Clone, Debug, PartialEq)]
pub struct EdgeIn {
    pub signal: String,
    pub target: NodeTarget,
    /// Suggested and not kept yet.
    pub pending: bool,
}

/// The edges the chat field shows, handed to the node view each frame.
/// `focus`: the chat field has keyboard focus. `batch`: id of the current
/// pending batch, 0 when none; changes when a new batch arrives.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Routing {
    pub edges: Vec<EdgeIn>,
    pub focus: bool,
    pub batch: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_default_routing_has_no_batch() {
        let r = Routing::default();
        assert!(r.edges.is_empty() && !r.focus && r.batch == 0);
        let e = EdgeIn { signal: "cam0.hand.left.spread".into(), target: NodeTarget::Macro(3), pending: true };
        assert_eq!(e.clone(), e);
        assert_ne!(NodeTarget::Part { id: 1, name: "Kick".into() }, NodeTarget::Param { plugin: "Serum".into(), name: "Kick".into(), inst: "gen:0:Serum".into() });
    }

    fn target(key: fl::Key, track: u32) -> fl::Target {
        fl::Target { key, track, track_name: String::new() }
    }

    /// Two Serum 2 instances name their tracks; a single plugin stays bare.
    #[test]
    fn twins_name_their_place() {
        use crate::i18n::{DE, EN};
        let gen = |ch| fl::Key { at: fl::At::Gen { ch }, plugin: "Serum 2".into() };
        let fx = |track| fl::Key { at: fl::At::Fx { track, slot: 0 }, plugin: "Serum 2".into() };
        let portal = fl::Key { at: fl::At::Fx { track: 4, slot: 1 }, plugin: "Portal".into() };
        let inv = [target(gen(2), 3), target(gen(9), 10), target(portal.clone(), 4)];
        let n = Names::new([], &inv);
        assert_eq!(n.label(&gen(2), &EN), "Serum 2 (Track 3)");
        assert_eq!(n.label(&gen(9), &EN), "Serum 2 (Track 10)");
        assert_eq!(n.label(&gen(9), &DE), "Serum 2 (Spur 10)");
        assert_eq!(n.label(&portal, &EN), "Portal", "one instance: no place");
        // a generator on no track names its channel; an effect its track
        let n = Names::new([&gen(5), &fx(0), &fx(7)], &[target(gen(5), 0)]);
        assert_eq!(n.label(&gen(5), &EN), "Serum 2 (Ch 6)");
        assert_eq!(n.label(&gen(5), &DE), "Serum 2 (Kanal 6)");
        assert_eq!(n.label(&fx(0), &EN), "Serum 2 (Master)");
        assert_eq!(n.label(&fx(7), &EN), "Serum 2 (Track 7)");
        // the routes alone make twins; the same key twice does not
        assert!(Names::new([&gen(2), &gen(9)], &[]).is_twin("Serum 2"));
        assert!(!Names::new([&gen(2), &gen(2)], &[target(gen(2), 3)]).is_twin("Serum 2"));
        let p = |plugin: &str, inst: &str| NodeTarget::Param { plugin: plugin.into(), name: "Filter 1 Freq".into(), inst: inst.into() };
        assert!(p("Serum 2", "gen:2:Serum 2").same(&p("Serum 2 (Track 3)", "gen:2:Serum 2")), "the label does not count");
        assert!(!p("Serum 2", "gen:2:Serum 2").same(&p("Serum 2", "gen:9:Serum 2")), "the instance does");
    }
}
