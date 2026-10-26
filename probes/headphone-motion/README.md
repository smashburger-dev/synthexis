# headphone-motion — Checkliste für Noa (P4)

1. Powerbeats Pro 2 **als Audio-Output des Macs verbinden** (Bluetooth).
2. Beide Buds tragen.
3. `open probes/headphone-motion/HeadphoneMotion.app` (oder Binary direkt).
4. Beim ersten Start: Motion-Freigabe-Dialog erlauben, falls gefragt.
5. 30 s laufen lassen; dabei Kopf links/rechts drehen und rauf/runter nicken.
6. `raw_motion.log` neben dem Binary an mich zurück.
7. Checken: connect-Event geloggt? yaw/pitch/roll-Werte ändern sich? Hz am
   Ende.
8. optional: Buds mid-run absetzen → disconnect-Event prüfen.

Headless verifiziert (ohne Buds): baut, startet, loggt
`authorizationStatus=notDetermined`, `isDeviceMotionAvailable=true`,
sammelt sauber (siehe `raw_motion.log` im App-Bundle).
