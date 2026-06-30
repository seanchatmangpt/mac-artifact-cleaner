# Chapter 12: Time Machine and APFS Snapshots

## 12.1 APFS Local Snapshots and Block Pinning
On macOS, files deleted from the active filesystem may not immediately free physical blocks if they are referenced by local APFS snapshots (e.g., those created by Time Machine). Deleting files in this state is not a real reclaim; the blocks remain pinned in storage.

## 12.2 Volumetric Reclaim Verification
To address this block pinning, `osx-clnr` implements a volumetric validation algorithm:
* **The Reclaim Delta Law:** We sample the available disk bytes before ($B_b$) and after ($B_a$) execution. The claimed bytes deleted $C$ must be witnessed by the physical space delta:
  $$\Delta = B_a - B_b$$
  We enforce the witness condition:
  $$\Delta \ge 0.5 \times C$$
  for claims greater than 1 GB. If $\Delta$ falls short, a `Shortfall` is reported, and the receipt fails verification. The remedy is thinning local APFS snapshots using `tmutil` to unpin the blocks.
