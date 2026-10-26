# heart-rate-ble — Checkliste für Noa (P5)

1. Powerbeats Pro 2 mit dem **iPhone koppeln** und in iOS
   Einstellungen → Bluetooth → Powerbeats Pro 2 → **"Herzfrequenz"/Heart
   Rate aktivieren** (Apple-Support: "Monitor your heart rate with
   Powerbeats Pro 2" — HR-Sharing läuft über diese Einstellung; genauen
   Menüpunkt im iOS deiner Version bestätigen).
2. Beide Buds einsetzen (HR wird nur getragen gemessen; manche Builds
   senden auch nur während eines Workouts in einer Fitness-App — wenn
   nichts kommt: Workout auf dem iPhone starten).
3. `open probes/heart-rate-ble/HeartRateBLE.app`.
4. Bluetooth-Dialog erlauben.
5. 60 s laufen lassen. Log: gefundene Powerbeats-Peripherals + advertised
   Services, connect, 2A37-Subscribe, HR bpm Zeilen.
6. `raw_hr.log` neben dem Binary zurückschicken.

Headless verifiziert (ich): baut, startet, `central state=5` (poweredOn),
Scan läuft ungefiltert 60 s; ohne getragene Buds wurde kein 180D-
Advertisement gesehen (erwartbar).
