# Voice catalog

The app offers one voice, defined in `src/voices.rs`. Earlier voices (eight Kitten aliases, Kokoro Heart, experimental Python models) were removed on 2026-11-03 and remain in git history.

| Display name | Information | Source | Index |
| --- | --- | --- | --- |
| Heart | American English · Female | Paradee v1.2 `af_heart`, speaker 0 | 0 |

Paradee v1.2 (`sahilmahendrakar/Paradee-8M-v1.2`, revision `ed2934d4df8a4fd891e29584c5e1104649409765`) is a 7.88M-parameter model distilled from Kokoro-82M. Its Heart voice is learned from Kokoro's `af_heart`; it is not Kokoro's own output. The app runs the 8,462,119-byte int8 graph through ONNX Runtime. The download is 21,089,306 bytes, including the Misaki pronunciation dictionaries; every file is hash-verified, and the generated token table and lexicons have pinned hashes too.

The graph has four voices. Isabella (British), Puck and Fable were auditioned and rejected; Isabella was judged robotic through the app even after its pronunciation matched Misaki. The frontend keeps British pronunciation support, which loads only if a British voice is selected.

Settings saved with a removed voice load as Heart.
