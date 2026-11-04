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
    // the Head section (head_ui.rs)
    h_title: "Head", "Kopf";
    h_head: "Head", "Kopf";
    h_eye_l: "Left eye", "Linkes Auge";
    h_eye_r: "Right eye", "Rechtes Auge";
    h_mouth: "Mouth", "Mund";
    h_pick: "Click the mouth, an eye or the head to see what it drives.", "Klick den Mund, ein Auge oder den Kopf an, um zu sehen, was er steuert.";
    h_no_face: "No face in view", "Kein Gesicht im Bild";
    h_ahead: "Facing ahead", "Blick geradeaus";
    h_turned_left: "Turned left", "Nach links gedreht";
    h_turned_right: "Turned right", "Nach rechts gedreht";
    h_looking_up: "Looking up", "Blick nach oben";
    h_looking_down: "Looking down", "Blick nach unten";
    h_tilted_left: "Tilted left", "Nach links geneigt";
    h_tilted_right: "Tilted right", "Nach rechts geneigt";
    h_mouth_open: "Mouth open", "Mund offen";
    h_eye_l_closed: "Left eye closed", "Linkes Auge zu";
    h_eye_r_closed: "Right eye closed", "Rechtes Auge zu";
    // a face or head signal in a Head list row ("Open → Serum 2 · Drive")
    g_open: "Open", "Offen";
    g_closed: "Closed", "Geschlossen";
    g_smile: "Smile", "Lächeln";
    g_turn: "Turn", "Drehen";
    g_nod: "Nod", "Nicken";
    g_head_tilt: "Tilt", "Neigen";
    // the tracking boxes over the camera (editor.rs crt_boxes)
    crt_head: "HEAD", "KOPF";
    crt_eye_l: "EYE L", "AUGE L";
    crt_eye_r: "EYE R", "AUGE R";
    crt_mouth: "MOUTH", "MUND";
    h_ex_head: "Turn my head to pan left and right · Tilt my head to sweep the cutoff · Nod my head to play a chord", "Wenn ich den Kopf drehe, pann nach links und rechts · Wenn ich den Kopf neige, fahr den Cutoff · Wenn ich nicke, spiel einen Akkord";
    h_ex_mouth: "When I open my mouth, open the filter · Open my mouth to play a snare", "Wenn ich den Mund öffne, öffne den Filter · Wenn ich den Mund aufmache, spiel eine Snare";
    h_ex_eye_l: "Close my left eye to play the kick pattern · My left eye opens the filter", "Wenn ich das linke Auge schließe, spiel die Kick · Das linke Auge steuert Signal 5";
    h_ex_eye_r: "Wink right to play the hi-hat pattern · Closing my right eye lowers the volume", "Zwinker rechts für die Hi-Hat · Das rechte Auge steuert Signal 6";
    // what fingers play (fingers.rs)
    f_title: "Hands", "Hände";
    f_single: "Single", "Einzeln";
    f_total: "All", "Alle";
    f_nothing: "{} plays nothing yet:", "{} spielt noch nichts:";
    f_pattern: "Pattern", "Pattern";
    f_chord: "Chord", "Akkord";
    f_held: "Held note", "Gehaltene Note";
    f_held_hint: "Holds while the finger plays. Drag its name up or down to change the note.", "Klingt, solange der Finger spielt. Den Namen hoch oder runter ziehen ändert die Note.";
    f_empty: "Empty", "Leer";
    f_no_channel: "Pick a channel", "Kanal wählen";
    f_not_linked: "FL not linked", "FL nicht verbunden";
    f_not_linked_hover: "The channels come from FL once it runs with a project open", "Die Kanäle kommen aus FL, sobald dort ein Projekt offen ist";
    f_while_held: "While shown", "Solange gezeigt";
    f_toggle: "On / off", "An / aus";
    f_once: "Once", "Einmal";
    f_start_now: "At once", "Sofort";
    f_start_16: "Next 1/16", "Nächstes 1/16";
    f_start_beat: "Next beat", "Nächster Beat";
    f_start_bar: "Next bar", "Nächster Takt";
    f_roll: "Piano roll", "Piano Roll";
    f_lanes: "Lanes", "Zeilen";
    f_roll_hide: "Hide roll", "Roll ausblenden";
    f_roll_show: "Show roll", "Roll einblenden";
    f_lane_vel: "Velocity", "Anschlag";
    f_hits: "{} hits", "{} Schläge";
    f_remove: "Remove", "Entfernen";
    f_bars: "bars", "Takte";
    f_fill: "Fill", "Füllen";
    f_add_row: "Add row", "Zeile hinzufügen";
    f_size: "Notes", "Töne";
    f_octave: "Octave", "Oktave";
    f_strum: "Strum", "Strum";
    f_vel: "Velocity", "Anschlag";
    f_chord_hint: "Click a chord to hold it, Shift-click adds it as the next bar. Editing a note in the roll makes the chord a pattern.", "Klick hält einen Akkord, Shift-Klick hängt ihn als nächsten Takt an. Eine Note in der Roll bearbeiten macht aus dem Akkord ein Pattern.";
    f_roll_hint: "Click adds a note, drag moves it, drag its right edge to lengthen it, right-click or right-drag deletes. Hold Option/Alt while dragging to move off the grid.",
        "Klick setzt eine Note, Ziehen verschiebt sie, ihre rechte Kante ziehen verlängert sie, Rechtsklick oder Rechts-Ziehen löscht. Mit Option/Alt beim Ziehen frei vom Raster verschieben.";
    f_scroll_hint: "Scroll over the roll for higher or lower notes; a double-click on the keys brings the notes into view.",
        "Über der Roll scrollen zeigt höhere oder tiefere Noten; ein Doppelklick auf die Tasten holt die Noten ins Bild.";
    f_lane_hint: "Drag a row's name up or down, or scroll on it, to change its note. Click it for a list.",
        "Den Namen einer Zeile hoch- oder runterziehen oder darauf scrollen ändert ihre Note. Ein Klick öffnet die Liste.";
    f_now_pattern: "Now a pattern: it loops instead of holding.", "Jetzt ein Pattern: es läuft in Schleife, statt zu halten.";
    f_back: "Back to {}", "Zurück zu {}";
    f_sampler_root: "C5 · Sampler root", "C5 · Sampler-Grundton";
    f_remove_row: "Remove row", "Zeile entfernen";
    f_calibrate: "Calibrate fingers", "Finger kalibrieren";
    f_cal_running: "Calibration running", "Kalibrierung läuft";
    f_cal_reset: "Reset", "Zurücksetzen";
    f_cal_show: "Both hands, hold: {}", "Beide Hände, halten: {}";
    f_cal_done: "Fingers calibrated", "Finger kalibriert";
    f_cal_hard: "hard to tell apart: {}", "schwer zu trennen: {}";
    f_cal_none: "No hand seen, nothing changed", "Keine Hand gesehen, nichts geändert";
    f_pose_fist: "fist", "Faust";
    f_pose_open: "open hand", "offene Hand";
    f_pose_alone: "{} alone", "nur {}";
    signals: "Signals 1–32 (Link to controller)", "Signale 1–32 (Link to controller)";
    n_shape_midi: "a hand shape plays MIDI (pattern, chord, note)", "eine Handform spielt MIDI (Pattern, Akkord, Note)";
    n_no_channel: "pick its FL channel under Hands", "FL-Kanal unter Hände wählen";
    midi_kept: "kept: see Hands", "behalten: siehe Hände";
    key_set: "MIDI key: {}", "MIDI-Tonart: {}";
    advanced: "Advanced", "Erweitert";
    // main view
    live: "Live", "Live";
    test_signal: "Test signal", "Testsignal";
    hand_left: "Left hand", "Linke Hand";
    hand_right: "Right hand", "Rechte Hand";
    settings: "⚙ Settings", "⚙ Einstellungen";
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
        "Die Vorschläge im Satzfeld folgen der Sprache. Du kannst in beiden Sprachen tippen.";
    set_typing: "Typing", "Tippen";
    completion: "Suggestions", "Vorschläge";
    completion_note: "Tab takes the grey suggestion, the arrow keys pick from the list.",
        "Tab übernimmt den grauen Vorschlag, die Pfeiltasten wählen aus der Liste.";
    scope_note: "These settings belong to your user account and apply to every synthexis instance.",
        "Diese Einstellungen gelten für dein Benutzerkonto und jede synthexis-Instanz.";
    diagnostics: "Diagnostics", "Diagnose";
    face_overlay: "Face overlay", "Gesichts-Overlay";
    fo_eyes: "Eyes", "Augen";
    fo_values: "Show values", "Werte zeigen";
    fo_note: "The boxes over the camera while a face is in view.", "Die Rahmen über dem Kamerabild, solange ein Gesicht im Bild ist.";
    diag_access: "Camera permission: {}", "Kamerafreigabe: {}";
    diag_camera: "Camera: {} fps · {} frames received · {} dropped", "Kamera: {} fps · {} Bilder empfangen · {} verworfen";
    diag_delay: "Frame delay, capture to callback: median {} ms, p95 {} ms", "Bildverzögerung, Aufnahme bis Callback: Median {} ms, p95 {} ms";
    diag_log: "Log (~/Library/Logs/synthexis.log)", "Log (~/Library/Logs/synthexis.log)";
    // plugin picker
    plugin: "Plugin", "Plugin";
    follow_fl: "Follow FL's focus", "FL-Fokus folgen";
    follow_fl_hover: "the plugin window last clicked in FL", "das zuletzt in FL angeklickte Plugin-Fenster";
    sound: "Sound", "Sound";
    fx: "FX", "FX";
    pin_gone: "{} is gone, following FL again", "{} ist weg, folge wieder FL";
    n_params: "{} parameters", "{} Parameter";
    reading_classes: "Reading parameter classes", "Lese Parameterklassen";
    fl_waiting: "Waiting for FL", "Warte auf FL";
    fl_no_answer: "FL does not answer. Restart FL once after installing synthexis. Still nothing: in Options > MIDI settings give the input \"synthexis\" the controller type \"synthexis bridge\" and the output \"synthexis\" the same port.",
        "FL antwortet nicht. FL nach der Installation von synthexis einmal neu starten. Danach immer noch nicht: unter Options > MIDI settings dem Eingang \"synthexis\" den Controller-Typ \"synthexis bridge\" und dem Ausgang \"synthexis\" denselben Port geben.";
    fl_old_script: "FL runs an old bridge script ({}). Copy plugin/fl/device_synthexis_bridge.py into FL's Hardware folder.",
        "FL nutzt ein altes Bridge-Skript ({}). plugin/fl/device_synthexis_bridge.py in FLs Hardware-Ordner kopieren.";
    fl_no_plugins: "No plugins in the project", "Keine Plugins im Projekt";
    fl_click_plugin: "Click a plugin window in FL", "Klicke in FL ein Plugin-Fenster an";
    // sentence field
    placeholder: "When I spread my fingers, open the filter", "Wenn ich die Finger spreize, Filter auf";
    loading: "Loading suggestions", "Lade Vorschläge";
    loader_failed: "Suggestions not started: {}", "Vorschläge nicht gestartet: {}";
    no_data: "No suggestion data ({})", "Keine Vorschlagsdaten ({})";
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
    // routes under the sentence field
    keep: "Keep", "Behalten";
    discard: "Discard", "Verwerfen";
    flip_hover: "Flip the direction", "Richtung umdrehen";
    remove_hover: "Remove this route", "Route entfernen";
    unmap_arm: "Remove? Click again", "Entfernen? Nochmal klicken";
    parser_unavailable: "parser unavailable (see the log)", "Parser nicht verfügbar (siehe Log)";
    removed_n: "removed {} route(s)", "{} Route(n) entfernt";
    undid_n: "undid {} route(s)", "{} Route(n) zurückgenommen";
    clear_scoped: "clearing one movement, target or track is not live yet; say which route to unmap",
        "Löschen nach Bewegung, Ziel oder Spur geht noch nicht live; sag, welche Route weg soll";
    unmap_unknown_move: "unmap: that movement has no route, nothing removed", "entfernen: diese Bewegung hat keine Route, nichts entfernt";
    unmap_unknown_target: "unmap: that target is not a parameter here, nothing removed",
        "entfernen: dieses Ziel ist hier kein Parameter, nichts entfernt";
    unmap_say_what: "unmap: say which movement or target", "entfernen: sag, welche Bewegung oder welches Ziel";
    partly: "partly removed", "teilweise entfernt";
    // notes on a route line (assign.rs)
    n_which_move: "which movement?", "welche Bewegung?";
    n_which_eye: "which eye? say left or right", "welches Auge? sag links oder rechts";
    n_which_hand: "which hand? say left or right", "welche Hand? sag links oder rechts";
    n_not_live: "{} is not live yet", "{} geht noch nicht live";
    n_no_param: "no parameter to drive", "kein Parameter zum Steuern";
    n_mods_not_live: "{} not live yet", "{} geht noch nicht live";
    n_gate: "gate {}: {}", "Gate {}: {}";
    n_gate_shape: "gate {} (a hand shape) not live yet", "Gate {} (eine Handform) geht noch nicht live";
    // a source the camera does not measure (assign.rs live_source, unmeasured)
    n_stand_in: "the camera does not measure {}, so this uses {}", "die Kamera misst {} nicht, deshalb wird {} genutzt";
    n_unmeasured: "the camera does not measure {} yet; try {}", "die Kamera misst {} noch nicht; probier {}";
    n_or: "or", "oder";
    mv_hand_y: "hand height", "Handhöhe";
    mv_hand_x: "hand left-right", "Hand links-rechts";
    mv_hands_distance: "distance between the hands", "Abstand der Hände";
    mv_fist: "fist", "Faust";
    mv_openness: "hand openness", "Handöffnung";
    mv_pinch: "pinch", "Pinch";
    mv_roll: "hand turn", "Hand drehen";
    mv_mouth: "mouth open", "Mund öffnen";
    mv_head_tilt: "head tilt", "Kopf neigen";
    mv_head_turn: "head turn", "Kopf drehen";
    // the suggestions in the top section (sentence.rs routes_inline)
    keep_none: "Nothing here can run yet: {}", "Hier kann noch nichts laufen: {}";
    // words of a route line (resolve::Words)
    // "first": the side before the movement ("left hand openness"), else after it
    r_side_order: "first", "after";
    r_left: "left", "links";
    r_right: "right", "rechts";
    r_both: "both hands", "beide Hände";
    r_inverted: "inverted", "umgekehrt";
    r_no_param: "{} (no parameter)", "{} (kein Parameter)";
    r_range: "range {}-{} %", "Bereich {}-{} %";
    r_amount: "amount {} %", "Stärke {} %";
    r_steps: "steps", "Stufen";
    r_toggles: "toggles", "schaltet um";
    r_while_held: "while held", "solange gehalten";
    r_ramps: "ramps", "Rampe";
    r_curve: "{} curve", "Kurve {}";
    r_snappy: "snappy", "direkt";
    r_smooth: "smooth", "weich";
    r_smoothing: "{} smoothing", "Glättung {}";
    r_holds: "holds", "hält";
    r_freezes: "freezes", "friert ein";
    r_resets: "resets", "setzt zurück";
    r_quantize: "{} steps", "{} Stufen";
    r_centred: "centred", "mittig";
    r_while: "while {}", "solange {}";
    // agent fingers: the part editor of a Pong wall (fingers.rs)
    fw_top: "Top wall", "Obere Wand";
    fw_right: "Right wall", "Rechte Wand";
    fw_bottom: "Bottom wall", "Untere Wand";
    fw_left: "Left wall", "Linke Wand";
    fw_note: "Note", "Note";
    fw_hit_plays: "A hit plays", "Treffer spielt";
    fw_note_hint: "Each hit plays the note for the hit length. Drag its name up or down to change the note.",
        "Jeder Treffer spielt die Note für die Trefferlänge. Den Namen hoch oder runter ziehen ändert die Note.";
    fw_chord_hint: "Click a chord to play it on each hit, Shift-click adds the next one: hits take them in turn. Editing a note in the roll makes the chord a pattern.",
        "Klick wählt den Akkord für jeden Treffer, Shift-Klick hängt den nächsten an: Treffer spielen sie der Reihe nach. Eine Note in der Roll bearbeiten macht aus dem Akkord ein Pattern.";
    fw_now_pattern: "Now a pattern: a hit plays it once to its end.", "Jetzt ein Pattern: ein Treffer spielt es einmal bis zum Ende.";
    // end agent fingers
    // agent pong_ui: the Pong section, side view and canvas walls (pong_ui.rs)
    p_title: "Pong", "Pong";
    p_name_hint: "Preset name", "Preset-Name";
    p_loaded: "Loaded {}", "{} geladen";
    p_load_undone: "Back to the state before", "Stand von vorher ist zurück";
    p_saved: "Saved {}", "{} gespeichert";
    p_deleted: "Deleted {}", "{} gelöscht";
    p_builtin_name: "\"{}\" is a built-in preset, pick another name", "\"{}\" ist ein eingebautes Preset, nimm einen anderen Namen";
    p_need_name: "Type a name first", "Erst einen Namen eingeben";
    p_on: "On", "An";
    p_hand: "Shooting hand", "Schießende Hand";
    p_left: "Left", "Links";
    p_right: "Right", "Rechts";
    p_both: "Both", "Beide";
    p_speed: "Speed", "Tempo";
    p_bounces: "Bounces", "Abpraller";
    p_time: "Time limit", "Zeitlimit";
    p_max: "Max balls", "Bälle max";
    p_never: "Never", "Nie";
    p_secs: "{} s", "{} s";
    p_vel: "Harder shot plays louder", "Härterer Schuss spielt lauter";
    p_faster: "Hard shot flies faster", "Harter Schuss fliegt schneller";
    p_pause: "Mute the shooting hand's parts", "Parts der Schusshand stumm";
    p_test: "Test shot", "Testschuss";
    p_burst: "Pong burst ×16", "Pong-Salve ×16";
    p_live: "{} flying · {} hits · {} dropped", "{} unterwegs · {} Treffer · {} verworfen";
    p_sides_hint: "Click a wall to choose what a hit does.", "Klick eine Wand an und wähle, was ein Treffer macht.";
    p_off: "Off", "Aus";
    p_midi: "MIDI", "MIDI";
    p_param: "Parameter", "Parameter";
    p_target: "Pick a target", "Ziel wählen";
    p_signal: "Signal {}", "Signal {}";
    p_loading: "Loading the parameters of {}", "Parameter von {} werden geladen";
    p_no_plugin: "No plugin picked: Signal 1..32 only", "Kein Plugin gewählt: nur Signal 1..32";
    p_ramp: "Ramp", "Rampe";
    p_position: "Position", "Position";
    p_toggle: "Toggle", "Umschalten";
    p_rest: "Rest", "Ruhe";
    p_peak: "Peak", "Spitze";
    p_depth: "Depth from the shot", "Tiefe aus dem Schuss";
    p_glide: "Glide back", "Zurückgleiten";
    p_ms: "{} ms", "{} ms";
    p_link: "Link Signal {} in FL", "Signal {} in FL verknüpfen";
    p_part_gone: "Part gone", "Part fehlt";
    p_note: "Note", "Note";
    // end agent pong_ui
    // agent sentence: the sentence field scoped to a Pong side (sentence.rs, assign.rs)
    w_top: "top wall", "obere Wand";
    w_right: "right wall", "rechte Wand";
    w_bottom: "bottom wall", "untere Wand";
    w_left: "left wall", "linke Wand";
    w_off: "off", "aus";
    w_drop_scope: "Stop assigning this wall", "Wand nicht mehr zuweisen";
    w_no_target: "say what the wall does", "sag, was die Wand macht";
    w_ignored: "{} left out, a wall does one thing", "{} weggelassen, eine Wand macht eine Sache";
    w_no_channel: "plays on FL's focused channel once FL's channel list is there", "spielt auf dem Kanal mit FL-Fokus, sobald FL die Kanalliste schickt";
    // end agent sentence
    // agent ui-redesign: new texts of docs/design/ui-redesign.md section 6
    plugin_picker: "Plugin picker", "Plugin-Auswahl";
    picker_following: "Following FL's focus", "Folgt dem FL-Fokus";
    preset_picker: "Preset picker", "Preset-Auswahl";
    preset_hands: "Hands", "Hände";
    preset_pong: "Pong", "Pong";
    preset_head: "Head", "Kopf";
    preset_builtin: "Built-in", "Eingebaut";
    preset_saved: "Saved presets", "Gespeicherte Presets";
    preset_saved_twin: "{} (saved)", "{} (gespeichert)";
    preset_edited: "{}, edited", "{}, geändert";
    save_as: "Save as...", "Speichern als...";
    save: "Save", "Speichern";
    overwrite: "Overwrite", "Überschreiben";
    undo: "Undo", "Zurück";
    undo_hover: "Undo: back to before {}", "Zurück: Stand vor {}";
    delete: "Delete", "Löschen";
    delete_arm: "Delete? Click again", "Löschen? Nochmal klicken";
    add_section: "Add a section", "Bereich hinzufügen";
    section_open: "Already open", "Schon offen";
    close_section: "Close {}. What it plays is removed; Undo brings it back.", "{} schließen. Was er spielt, wird entfernt; Zurück holt es wieder.";
    close_arm_hands: "Close Hands? {} parts go", "Hände schließen? {} Parts werden entfernt";
    close_arm_pong: "Close Pong? Walls and Pong settings go", "Pong schließen? Wände und Pong-Einstellungen werden entfernt";
    close_arm_head: "Close Head? {} routes go", "Kopf schließen? {} Routen werden entfernt";
    replace_arm: "{} goes. Click again", "{} wird entfernt. Nochmal klicken";
    section_closed: "Closed {}", "{} geschlossen";
    section_opened: "Opened {}", "{} geöffnet";
    reloaded: "Reloaded {}", "{} neu geladen";
    ov_pending: "Suggested routes: {}", "Vorgeschlagene Routen: {}";
    ov_kept: "Kept routes: {}", "Behaltene Routen: {}";
    ov_reading: "Reading the sentence", "Satz wird gelesen";
    ov_show: "Show the routes", "Routen zeigen";
    ov_hide: "Hide the routes", "Routen ausblenden";
    ov_kept_head: "Kept", "Behalten";
    keys_note: "Cmd+Enter keeps the suggested routes, Cmd+Backspace discards them.", "Cmd+Enter behält die vorgeschlagenen Routen, Cmd+Rückschritt verwirft sie.";
    f_routes: "Routes", "Routen";
    f_single_hover: "One finger's part", "Der Part eines Fingers";
    f_all_hover: "All parts at once", "Alle Parts auf einmal";
    f_routes_hover: "Where the hands go", "Wohin die Hände gehen";
    f_routes_empty: "No routes from the hands yet. Type one in the field above.", "Noch keine Routen von den Händen. Schreib oben eine.";
    f_param: "Parameter", "Parameter";
    f_param_none: "Not routed yet. Type it in the chat above.", "Noch nicht geroutet. Schreib es oben in den Chat.";
    f_param_pending: "suggested", "vorgeschlagen";
    f_n_params: "{} parameters", "{} Parameter";
    f_n_routes: "{} routes", "{} Routen";
    f_hand_drives: "{} drives:", "{} steuert:";
    f_fist: "Fist", "Faust";
    f_open: "Open hand", "Offene Hand";
    f_shape_hover: "Plays while the whole hand shows this shape", "Spielt, solange die ganze Hand diese Form zeigt";
    f_older_shapes: "+{} older shapes", "+{} ältere Formen";
    f_and: "and", "und";
    f_pick_part: "Click a finger or a hand shape to give it a part.", "Klick einen Finger oder eine Handform an, um ihm einen Part zu geben.";
    f_template: "Template", "Vorlage";
    f_saved_parts: "Saved parts", "Gespeicherte Parts";
    f_part_name: "Part name", "Part-Name";
    f_no_parts: "No saved parts yet", "Noch keine gespeicherten Parts";
    f_repeats: "Repeats", "Wiederholungen";
    // finger calibration notes (calib::Outcome::misread, the stalled progress)
    f_cal_misread: "{} reads as {}", "{}: die Kamera liest {}";
    f_cal_alone: "{} alone", "{} allein";
    cal_thumb: "thumb", "Daumen";
    cal_index: "index", "Zeigefinger";
    cal_middle: "middle", "Mittelfinger";
    cal_ring: "ring", "Ringfinger";
    cal_pinky: "pinky", "kleiner Finger";
    f_cal_no_hand: "No hand in view", "Keine Hand im Bild";
    g_openness: "Openness", "Öffnung";
    g_spread: "Spread", "Spreizung";
    g_x: "Left-right", "Links-rechts";
    g_y: "Up-down", "Hoch-runter";
    g_depth: "Distance", "Abstand";
    g_pinch: "Pinch", "Pinch";
    g_speed: "Hand speed", "Handtempo";
    g_roll: "Turn", "Drehung";
    g_tilt: "{} tilt", "{} Neigung";
    g_count: "Finger count", "Fingerzahl";
    g_shown: "{} shown", "{} gestreckt";
    g_more: "+{} more", "+{} weitere";
    g_signals: "Signals", "Signale";
    g_parts: "Parts", "Parts";
    // the search over parameters, Signals and movements (param_find.rs)
    pf_hint: "Type to search", "Tippen zum Suchen";
    pf_none: "No match", "Kein Treffer";
    pf_more: "Type to see more", "Tippen für mehr";
    pf_target_hover: "Pick another target", "Anderes Ziel wählen";
    pf_source_hover: "Pick another movement", "Andere Bewegung wählen";
    set_calibration: "Finger calibration", "Fingerkalibrierung";
    set_calibrated: "Calibrated", "Kalibriert";
    set_not_calibrated: "Not calibrated", "Nicht kalibriert";
    set_project: "Saved with the project", "Mit dem Projekt gespeichert";
    p_live_off: "Live is off", "Live ist aus";
    p_test_off: "Turn Pong and Live on to test", "Pong und Live einschalten zum Testen";
    p_speed_unit: "{} screens/s", "{} Bildhöhen/s";
    // the profiles 1 2 3 in the top bar (preset/profile.rs)
    profile_now: "Profile {}: what is set up now", "Profil {}: was jetzt eingerichtet ist";
    profile_go: "Profile {}. Click to switch; what is set up now stays in profile {}.",
        "Profil {}. Klicken zum Wechseln; was jetzt eingerichtet ist, bleibt in Profil {}.";
    profile_go_empty: "Profile {}: empty. Click to set it up fresh; what is set up now stays in profile {}.",
        "Profil {}: leer. Klicken, um es neu einzurichten; was jetzt eingerichtet ist, bleibt in Profil {}.";
    profile_switched: "Profile {}", "Profil {}";
    profile_fresh: "Profile {}: empty, set it up fresh", "Profil {}: leer, neu einrichten";
    // end agent ui-redesign
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

    /// The words of a route line; German names the side after the movement
    /// ("hand openness (links)"), since movement ids stay English.
    pub fn words(&self) -> synthexis_parser::resolve::Words {
        synthexis_parser::resolve::Words {
            left: self.r_left,
            right: self.r_right,
            both_hands: self.r_both,
            side_first: self.r_side_order == "first",
            inverted: self.r_inverted,
            no_parameter: self.r_no_param,
            range: self.r_range,
            amount: self.r_amount,
            steps: self.r_steps,
            toggles: self.r_toggles,
            while_held: self.r_while_held,
            ramps: self.r_ramps,
            curve: self.r_curve,
            snappy: self.r_snappy,
            smooth: self.r_smooth,
            smoothing: self.r_smoothing,
            holds: self.r_holds,
            freezes: self.r_freezes,
            resets: self.r_resets,
            quantize: self.r_quantize,
            centred: self.r_centred,
            while_: self.r_while,
        }
    }

    /// Finger names in panel order (synthexis_complete::FINGERS).
    pub fn fingers(&self) -> [&'static str; 5] {
        [self.thumb, self.index, self.middle, self.ring, self.pinky]
    }

    /// The whole finger names for running text (the calibration's
    /// misread line), in panel order: "Mittelfinger", not the chip's
    /// "Mittel".
    pub fn fingers_full(&self) -> [&'static str; 5] {
        [self.cal_thumb, self.cal_index, self.cal_middle, self.cal_ring, self.cal_pinky]
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

    /// Keys whose text stands alone on a control, list row, title, header,
    /// placeholder or message line: sentence case (ui-redesign 2.3). Route
    /// words, wall names in a line and notes after " . " stay lowercase and
    /// are not listed.
    const STANDALONE: &[&str] = &[
        // new in v2
        "plugin_picker", "picker_following", "preset_picker", "preset_hands", "preset_pong", "preset_builtin", "preset_saved",
        "preset_saved_twin", "preset_edited", "save_as", "save", "overwrite", "undo", "undo_hover", "delete", "delete_arm",
        "add_section", "section_open", "close_section", "close_arm_hands", "close_arm_pong", "replace_arm", "section_closed",
        "section_opened", "reloaded", "ov_pending", "ov_kept", "ov_reading", "ov_show", "ov_hide", "ov_kept_head", "keys_note",
        "f_routes", "f_single_hover", "f_all_hover", "f_routes_hover", "f_routes_empty", "f_fist", "f_open", "f_shape_hover",
        "f_older_shapes", "f_pick_part", "f_param", "f_param_none", "f_n_params", "f_n_routes", "f_hand_drives", "f_template", "f_saved_parts", "f_part_name", "f_no_parts", "f_repeats",
        "f_cal_misread", "f_cal_no_hand", "f_cal_running", "f_not_linked_hover", "g_openness", "g_spread", "g_x", "g_y", "g_depth", "g_pinch", "g_speed", "g_roll",
        "g_tilt", "g_count", "g_shown", "g_more", "g_signals", "g_parts", "set_calibration", "set_calibrated",
        "pf_hint", "pf_none", "pf_more", "pf_target_hover", "pf_source_hover",
        "set_not_calibrated", "set_project", "p_on", "p_live_off", "p_test_off", "p_speed_unit",
        // changed in v2
        "f_title", "follow_fl", "placeholder", "flip_hover", "remove_hover", "w_drop_scope", "fl_no_plugins",
        "fl_click_plugin", "fl_waiting", "loading", "loader_failed", "no_data", "reading_classes", "f_no_channel",
        "f_while_held", "f_toggle", "f_once", "f_start_now", "f_start_16", "f_start_beat", "f_start_bar", "f_size",
        "f_octave", "f_strum", "f_vel", "f_add_row", "fw_hit_plays", "p_speed", "p_bounces", "p_time", "p_max", "p_never",
        "p_vel", "p_faster", "p_pause", "p_burst", "p_target", "p_rest", "p_peak", "p_depth", "p_glide", "p_link",
        "p_part_gone", "p_loading", "p_no_plugin", "p_name_hint", "p_loaded", "p_saved", "p_deleted", "p_load_undone",
        "p_need_name",
        // the Head section
        "preset_head", "close_arm_head", "h_title", "h_head", "h_eye_l", "h_eye_r", "h_mouth", "h_pick", "h_no_face", "h_ahead",
        "h_turned_left", "h_turned_right", "h_looking_up", "h_looking_down", "h_tilted_left", "h_tilted_right", "h_mouth_open",
        "h_eye_l_closed", "h_eye_r_closed",
        "g_open", "g_closed", "g_smile", "g_turn", "g_nod", "g_head_tilt",
        // Settings' face overlay, the drives lists' x
        "face_overlay", "fo_eyes", "fo_values", "fo_note", "unmap_arm", "settings_title", "language", "set_typing", "completion", "diagnostics",
        // the profiles
        "profile_now", "profile_go", "profile_go_empty", "profile_switched", "profile_fresh",
    ];

    /// The first letter is a capital, unless a value leads ("{} geladen":
    /// the name keeps its own case).
    fn starts_upper(s: &str) -> bool {
        let s = s.trim_start_matches('+');
        s.starts_with("{}") || s.chars().find(|c| c.is_alphabetic()).is_some_and(char::is_uppercase)
    }

    #[test]
    fn ui_labels_start_uppercase() {
        assert!(!starts_upper("follows FL") && starts_upper("{} geladen") && starts_upper("+{} more") && starts_upper("Next 1/16"));
        for t in [&EN, &DE] {
            let entries = t.entries();
            for key in STANDALONE {
                let text = entries.iter().find(|(k, _)| k == key).map(|(_, v)| *v);
                assert!(text.is_some_and(starts_upper), "{key}: {text:?}");
            }
        }
    }

    #[test]
    fn fill_puts_values_in_order() {
        assert_eq!(fill(EN.diag_camera, &[&30, &1234, &0]), "Camera: 30 fps · 1234 frames received · 0 dropped");
        assert_eq!(fill("{} and {}", &[&1]), "1 and ");
        assert_eq!(fill("none", &[&1]), "none");
    }
}
