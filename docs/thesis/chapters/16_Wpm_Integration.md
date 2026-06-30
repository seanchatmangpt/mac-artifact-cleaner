# Chapter 16: Wpm Integration

## 16.1 The Wasm4pm Execution Engine
To support secure, cross-platform process mining analytics, `osx-clnr` integrates with the `wasm4pm` execution engine. `wasm4pm` compiles conformance checking, process discovery, and audit rules into lightweight WebAssembly modules.

## 16.2 Object-Centric Petri Net (OCPN) Replay
The alignment-based conformance checking is executed by replaying the OCEL logs onto the OCPN structure inside the WASM sandbox. This ensures isolation and prevents execution side-effects. The replay algorithm calculates trace fitness by mapping event sequences to transitions and identifying token shortfalls or overflows in the Petri Net places.
