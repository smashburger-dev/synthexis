//! Every string the plugin window shows, in English and German, switched
//! by the language setting (settings.rs). Each entry names both texts side
//! by side, so a string without a translation does not compile. "{}" marks
//! where fill() puts a value. Host-visible parameter names ("Live", "Signal
//! 1") and FL's own terms in plugin places ("Master", "Track 3", "Channel
//! 1", "slot") stay English in both languages: FL shows them that way.

use synthexis_complete::Lang;

macro_rules! texts {
    ($($name:ident: $en:expr, $de:expr;)*) => {
        pub struct Text {
            $(pub $name: &'static str,)*
        }

        pub const EN: Text = Text { $($name: $en,)* };
        pub const DE: Text = Text { $($name: $de,)* };

        impl Text {
            /// (name, text) of every entry, for the tests.
            #[cfg(test)]
            pub fn entries(&self) -> Vec<(&'static str, &'static str)> {
                vec![$((stringify!($name), self.$name),)*]
            }
        }
    };
}

texts! {
    // main view
    live: "Live", "Live";
    test_signal: "Test signal", "Testsignal";
    hand_left: "Left hand", "Linke Hand";
    hand_right: "Right hand", "Rechte Hand";
    role_midi: "MIDI", "MIDI";
    role_parameter: "Parameter", "Parameter";
    role_motion: "Motion", "Bewegung";
    role_off: "Off", "Aus";
    settings: "⚙ Settings", "⚙ Einstellungen";
    assistant: "Mapping assistant", "Mapping-Assistent";
    cam_on: "Camera on · {} fps", "Kamera an · {} fps";
    cam_starting: "Camera starting", "Kamera startet";
    cam_asking: "Waiting for camera permission", "Warte auf die Kamerafreigabe";
    cam_denied: "No camera access. Allow it in System Settings > Privacy & Security > Camera, then reopen the plugin.",
        "Kein Kamerazugriff. In Systemeinstellungen > Datenschutz & Sicherheit > Kamera erlauben, dann das Plugin neu öffnen.";
    cam_no_picture: "The camera sends no picture (in use by another app?)", "Die Kamera liefert kein Bild (von einer anderen App belegt?)";
    // settings overlay
    settings_title: "Settings", "Einstellungen";
    close: "Close (Esc)", "Schließen (Esc)";
    language: "Language", "Sprache";
    language_note: "Suggestions in the sentence field follow the language. You can type in either language.",
        "Die Vorschläge im Satzfeld folgen der Sprache: Deutsch zeigt deutsche und englische. Tippen geht in beiden Sprachen.";
    completion: "Suggestions while typing", "Vorschläge beim Tippen";
    completion_note: "Tab takes the grey suggestion, the arrow keys pick from the list.",
        "Tab übernimmt den grauen Vorschlag, die Pfeiltasten wählen aus der Liste.";
    scope_note: "These settings belong to your user account and apply to every synthexis instance.",
        "Diese Einstellungen gelten für dein Benutzerkonto und jede synthexis-Instanz.";
    diagnostics: "Diagnostics", "Diagnose";
    diag_access: "Camera permission: {}", "Kamerafreigabe: {}";
    diag_camera: "Camera: {} fps · {} frames received · {} dropped", "Kamera: {} fps · {} Bilder empfangen · {} verworfen";
    diag_delay: "Frame delay, capture to callback: median {} ms, p95 {} ms", "Bildverzögerung, Aufnahme bis Callback: Median {} ms, p95 {} ms";
    diag_log: "Log (~/Library/Logs/synthexis.log)", "Log (~/Library/Logs/synthexis.log)";
    // plugin picker
    plugin: "Plugin", "Plugin";
    follows_fl: "follows FL", "folgt FL";
    follows_fl_after: "{} (follows FL)", "{} (folgt FL)";
    follow_fl: "Follow FL", "FL folgen";
    follow_fl_hover: "the plugin window last clicked in FL", "das zuletzt in FL angeklickte Plugin-Fenster";
    sound: "Sound", "Sound";
    fx: "FX", "FX";
    pin: "Pin", "Anheften";
    unpin: "Unpin", "Lösen";
    pin_gone: "{} is gone, following FL again", "{} ist weg, folge wieder FL";
    n_params: "{} parameters", "{} Parameter";
    reading_classes: "reading parameter classes", "lese Parameterklassen";
    fl_waiting: "waiting for FL", "warte auf FL";
    fl_no_answer: "FL does not answer. In FL, open Options > MIDI settings and give the input \"synthexis bridge\" the controller type \"synthexis bridge\".",
        "FL antwortet nicht. In FL unter Options > MIDI settings dem Eingang \"synthexis bridge\" den Controller-Typ \"synthexis bridge\" geben.";
    fl_old_script: "FL runs an old bridge script ({}). Copy plugin/fl/device_synthexis_bridge.py into FL's Hardware folder.",
        "FL nutzt ein altes Bridge-Skript ({}). plugin/fl/device_synthexis_bridge.py in FLs Hardware-Ordner kopieren.";
    fl_no_plugins: "no plugins in the project", "keine Plugins im Projekt";
    fl_click_plugin: "click a plugin window in FL", "klicke in FL ein Plugin-Fenster an";
    // sentence field
    placeholder: "when I spread my fingers, open the filter", "wenn ich die Finger spreize, Filter auf";
    loading: "loading suggestions", "lade Vorschläge";
    loader_failed: "suggestions not started: {}", "Vorschläge nicht gestartet: {}";
    no_data: "no suggestion data ({})", "keine Vorschlagsdaten ({})";
    midi_note: "MIDI note", "MIDI-Note";
    midi_chord: "MIDI chord", "MIDI-Akkord";
    midi_pattern: "MIDI pattern", "MIDI-Pattern";
    midi_cc: "MIDI CC", "MIDI-CC";
    midi_prop: "MIDI {}", "MIDI {}";
    macro_: "macro", "Makro";
    xy_pad: "XY pad", "XY-Pad";
    inverse: "{} (inverse)", "{} (umgekehrt)";
    // hand side and finger panel
    side_left: "left", "links";
    side_right: "right", "rechts";
    badge_left: "L", "L";
    badge_right: "R", "R";
    thumb: "thumb", "Daumen";
    index: "index", "Zeige";
    middle: "middle", "Mittel";
    ring: "ring", "Ring";
    pinky: "pinky", "Klein";
    side_open: "left/right?", "links/rechts?";
    finger_open: "finger?", "Finger?";
    panel_hand: "Hand", "Hand";
    panel_finger: "Finger", "Finger";
    keys_both: "← → hand   ↑ ↓ finger   Enter", "← → Hand   ↑ ↓ Finger   Enter";
    keys_side: "← left   → right   Enter", "← links   → rechts   Enter";
    keys_finger: "↑ ↓ finger   Enter", "↑ ↓ Finger   Enter";
}

pub fn text(lang: Lang) -> &'static Text {
    match lang {
        Lang::En => &EN,
        Lang::De => &DE,
    }
}

impl Text {
    /// Side names in panel order (synthexis_complete::SIDES).
    pub fn sides(&self) -> [&'static str; 2] {
        [self.side_left, self.side_right]
    }

    pub fn side_badges(&self) -> [&'static str; 2] {
        [self.badge_left, self.badge_right]
    }

    /// Finger names in panel order (synthexis_complete::FINGERS).
    pub fn fingers(&self) -> [&'static str; 5] {
        [self.thumb, self.index, self.middle, self.ring, self.pinky]
    }
}

/// `template` with each "{}" replaced by the next of `args`.
pub fn fill(template: &str, args: &[&dyn std::fmt::Display]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut args = args.iter();
    let mut rest = template;
    while let Some(i) = rest.find("{}") {
        out.push_str(&rest[..i]);
        if let Some(a) = args.next() {
            out.push_str(&a.to_string());
        }
        rest = &rest[i + 2..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_entry_has_both_texts_with_the_same_slots() {
        for ((name, en), (_, de)) in EN.entries().into_iter().zip(DE.entries()) {
            assert!(!en.trim().is_empty() && !de.trim().is_empty(), "{name} is empty");
            assert_eq!(en.matches("{}").count(), de.matches("{}").count(), "{name}: slots differ");
        }
    }

    #[test]
    fn english_texts_have_no_german() {
        let german = ["ä", "ö", "ü", "ß", "Ä", "Ö", "Ü", " und ", " der ", " die ", " das ", "Einstell"];
        for (name, en) in EN.entries() {
            assert!(!german.iter().any(|g| en.contains(g)), "{name}: {en:?}");
        }
    }

    #[test]
    fn panel_order_matches_the_ids() {
        assert_eq!(EN.sides(), synthexis_complete::SIDES);
        assert_eq!(EN.fingers(), synthexis_complete::FINGERS);
    }

    #[test]
    fn fill_puts_values_in_order() {
        assert_eq!(fill(EN.diag_camera, &[&30, &1234, &0]), "Camera: 30 fps · 1234 frames received · 0 dropped");
        assert_eq!(fill("{} and {}", &[&1]), "1 and ");
        assert_eq!(fill("none", &[&1]), "none");
    }
}
