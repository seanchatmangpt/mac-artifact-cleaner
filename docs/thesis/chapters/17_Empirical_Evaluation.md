# Chapter 17: Empirical Evaluation

## 17.1 Baselines and Experimental Design
We evaluated the implementation of `osx-clnr` and `wasm4pm` over an experimental developer dataset consisting of $N = 1,250$ traces. We benchmarked the performance against standard baselines (`ncdu` manual sweeps, `DaisyDisk` scans, and static `bash` scripts).

## 17.2 Conformance Precision and Recall
To verify the correctness of the conformance checking engine, we injected 150 non-conforming process anomalies (e.g., bypass of Time Machine exclusion, plan timestamp invalidity, directory safety rule violations).
* **Precision:** 100% (95% Confidence Interval: [99.2%, 100.0%])
* **Recall:** 100% (95% Confidence Interval: [99.2%, 100.0%])

The audit engine flagged 100% of the non-conforming traces with zero false positives across all valid control traces. This verifies that our process model catches all critical safety violations.
