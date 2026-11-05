# Submission texts

Three fields, each at most 3000 characters. Numbers come from the repo
(probes/*/RESULT.md, docs/roadmap.md, plugin/models/README.md).

## Project description: describe your project and what problem it solves

synthexis turns the webcam you already own into an instrument for the plugins you already love.

Every producer has the same problem. A synth like Serum 2 has more than 500 parameters, and you reach them one at a time with a mouse. Hardware controllers help, but they cost money, take up desk space, and all they add is more knobs. The most expressive thing in the studio is your own body, and today it does nothing.

synthexis is a plugin for FL Studio. Load it, look into your camera, and type what you want in plain English or German: "when I open my left hand, open the filter", "pinch my fingers to lower the reverb", "when I open my mouth, push the distortion". It understands the sentence, finds the right parameter in whatever plugin you picked, and from then on your hand, your fingers, your mouth, your eyes or the turn of your head play that parameter live. FL records it as automation, so a performance becomes part of the track.

It goes beyond knobs:
- Hands: each finger can trigger its own MIDI part, a hi-hat pattern on the index, chords on the ring finger, all in the key of the song.
- Head: turn, nod and tilt, mouth and each eye become controllers, shown as a live drawing of your face.
- Pong: point your finger and shoot a ball across the camera picture. Each of the four walls plays a note, a chord or moves a parameter when the ball hits it. Music as a game.
- Profiles and presets: switch between whole setups with one click on 1, 2 or 3.

Every plugin works. synthexis ships with readable parameter names for 887 plugins (107,350 labelled parameters), and for any plugin it has never seen, a small model running inside the plugin works out what each parameter does from its name.

Everything runs on your Mac, in real time, with no cloud and no extra hardware. From your movement to the parameter in FL takes about a tenth of a second.

The result: plugins you have used for years feel new again, you can perform them wherever you are with a laptop, and your expression is no longer limited to what fits under a mouse pointer.

## Describe your use of Meta products to build the submitted project

We used three Meta models to build the parts of synthexis that see and the parts that understand.

SAM 3.1 drew the masks our vision models learned from. We recorded four sessions of hands, faces, objects and movement, and SAM 3.1 segmented every frame: 47,367 frames with person, face, eye, mouth, hand and object masks. Its grouped image mode also found blurred hands in fast movement. From these masks we trained our own student models: a segmentation model and a face model, each about 1.5 million parameters, small enough to run on the Mac's Neural Engine in well under a millisecond per frame. The hand and face crops our other models train on were cut out with SAM 3.1 masks as well.

SAM 3D Body gave us the hands that are hardest to see. A first keypoint teacher missed many fists, hands that hold something and hands half hidden behind each other. SAM 3D Body filled 2,349 of those hands with keypoints and found 83 % of the fists in our first session, where the other teacher found 63 %. It also labelled which hand is left and which is right; with that, synthexis puts each hand on the right side in 98.6 % of the frames of a session it never trained on.

Muse Spark Open taught synthexis to understand plugins and sentences:
- Plugin parameters: it labelled what each parameter of hundreds of plugins does, with a class, a direction and a safe range. Priority plugins were labelled twice and the differences settled (16,509 lines for 302 Audio Unit plugins, 8,103 for 161 VST3 plugins, plus 11,719 lines for 583 more). These labels became ParamSense, the model in the plugin that reads any parameter name, and the table of 887 plugins that ships with it.
- Sentences: it wrote tens of thousands of training sentences for the parser that turns "pinch my fingers to lower the reverb" into a route, in English and German, plus a glossary of 11,333 phrases for 416 concepts.
- Quality checks: it helped label and review our recordings as a second pair of eyes.

The parser trained on this data reaches 65 % exact routes on real sentences people typed (66 of 102), and 87 % are right or one detail away; Muse Spark Open itself, asked directly, got 62 of 99. The parser runs inside the plugin in about 75 ms.

All three models are open, so we could train our own small models from them and ship those inside the plugin. The project is Built with Muse Spark Open.

## Why are Meta's models important to your solution?

synthexis only works if three hard things happen on a normal laptop, live, inside a music program: it has to see your body precisely, it has to know what every plugin parameter means, and it has to understand what you say. Meta's models made each of these possible.

Seeing. A plugin gets a few milliseconds per frame. The quality we needed for fingers, eyes and mouths comes from SAM 3.1, and SAM 3.1's masks let us build compact student models that carry that knowledge into the plugin: segmentation, hands and face together take about 1.6 ms of model time per frame on the Neural Engine. Without SAM 3.1 we would have had to draw tens of thousands of masks by hand; with it, four recording sessions became a full training set. SAM 3D Body added the poses every musician makes and most hand trackers lose: the closed fist, the hand around a drumstick, two hands crossing. Because it understands the hand as a 3D body part, it labelled fists and left and right hands where other tools gave up, and that is why opening your fist can sweep a filter smoothly.

Understanding plugins. There are thousands of plugins, each with its own naming. "Filter 1 Freq", "Cutoff", "FLT1 CUT" and "Brightness" can mean the same thing. Muse Spark Open read the parameter lists of hundreds of plugins and labelled what each control does, which way is up, and which range is safe to move. That turned 887 plugins into something synthexis can talk to, and trained ParamSense, so the next plugin, one nobody has labelled yet, works too.

Understanding people. Musicians do not speak in parameter IDs. They say "open the filter when I spread my fingers" or "Filter runter, wenn ich die Faust mache". Muse Spark Open wrote the varied, realistic sentences our parser learned from, in two languages, and was the benchmark we measured it against. The result is a parser small enough to live in the plugin and run offline.

Open weights matter as much as quality. Because SAM 3.1, SAM 3D Body and Muse Spark Open are open, we could build our own models from them, and run everything locally inside the plugin. A musician gets an instant instrument with no subscription and no extra hardware, and the plugins they already own become playable with their hands, their face and a typed sentence.
