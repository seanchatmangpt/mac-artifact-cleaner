# Chapter 15: Noun-Verb Substrate

## 15.1 Architectural Decomposition
The `osx-clnr` utility decouples its CLI interaction layer from core domain logic using the `clap-noun-verb` substrate. This decomposition models system entities as nouns (governed resources) and operations as verbs (allowed transitions).

## 15.2 Nouns and Verbs
* **Nouns:** `audit`, `artifact`, `tool-roots`, `plan`, `delete`, `receipt`, `snapshot`, `exclusion`, `ocel`, `privacy`, `doctor`.
* **Verbs:** `run`, `summarize`, `build`, `inspect`, `validate`, `execute`, `dry-run`, `verify`, `thin`, `apply`, `redact`.

By maintaining a thin CLI layer, the command parser delegates validation immediately to domain functions, preventing business policy from drifting into command-line wrapper scripts.
