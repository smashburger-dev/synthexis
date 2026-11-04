//! Routing edges shared by the chat field (sentence.rs) and the hands'
//! node view (fingers.rs): what a route connects, whether it still waits
//! for Keep, and the batch it came in (docs/design/ui-redesign.md, Unit 1
//! contract, R19).

/// What a route drives.
#[derive(Clone, Debug, PartialEq)]
pub enum NodeTarget {
    /// A plugin parameter, by plugin label and parameter name.
    Param { plugin: String, name: String },
    /// One of the 32 Signal macros (0-based).
    Macro(usize),
    /// A MIDI part, by its store id and name.
    Part { id: u64, name: String },
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
        assert_ne!(NodeTarget::Part { id: 1, name: "Kick".into() }, NodeTarget::Param { plugin: "Serum".into(), name: "Kick".into() });
    }
}
