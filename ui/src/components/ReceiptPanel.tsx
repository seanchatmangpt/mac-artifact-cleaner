import { getReceipt } from "@/data/oclnr";

export default async function ReceiptPanel() {
  const r = await getReceipt();

  if (!r) {
    return (
      <section className="space-y-2">
        <h2 className="text-lg font-semibold text-slate-200">Last Snapshot Receipt</h2>
        <p className="text-slate-500 text-sm">No snapshot-thin receipt found on disk.</p>
      </section>
    );
  }

  const requestedGb = (r.requested_bytes / 1e9).toFixed(0);
  const ts = new Date(r.timestamp_unix * 1000).toLocaleString();

  return (
    <section className="space-y-3">
      <div className="flex items-end justify-between">
        <h2 className="text-lg font-semibold text-slate-200">Last Snapshot Receipt</h2>
        <span className="text-xs text-slate-500">{ts}</span>
      </div>
      <div className="rounded border border-slate-700 bg-slate-800/50 p-4 space-y-3">
        <div className="grid grid-cols-3 gap-4">
          <div>
            <div className="text-2xl font-bold tabular-nums text-slate-100">{r.snapshots_thinned.length}</div>
            <div className="text-xs text-slate-400 mt-0.5">thinned</div>
          </div>
          <div>
            <div className="text-2xl font-bold tabular-nums text-slate-100">{r.snapshots_before.length}</div>
            <div className="text-xs text-slate-400 mt-0.5">before</div>
          </div>
          <div>
            <div className="text-2xl font-bold tabular-nums text-slate-100">{requestedGb} GB</div>
            <div className="text-xs text-slate-400 mt-0.5">requested</div>
          </div>
        </div>
        <div className="pt-2 border-t border-slate-700">
          <div className="text-xs text-slate-500 mb-1">Volume</div>
          <code className="text-xs font-mono text-slate-300">{r.volume}</code>
        </div>
        {r.snapshots_after.length === 0 && (
          <p className="text-xs text-green-400">All snapshots cleared after thin.</p>
        )}
      </div>
      <p className="text-xs text-slate-600">
        Read from <code className="font-mono">snapshot-thin-receipt.json</code> — written by{" "}
        <code className="font-mono">oclnr snapshot thin</code>
      </p>
    </section>
  );
}
