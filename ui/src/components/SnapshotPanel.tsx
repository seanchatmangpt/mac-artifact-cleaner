import { getSnapshots } from "@/data/oclnr";
import type { Snapshot } from "@/data/oclnr";

function SnapshotBadge({ s }: { s: Snapshot }) {
  const label =
    s.kind === "time-machine" ? `TM · ${s.date ?? ""}`
    : s.kind === "os-update" ? "OS Update"
    : "Other";
  const color =
    s.kind === "time-machine"
      ? "bg-blue-900/60 text-blue-300 border-blue-700"
      : s.kind === "os-update"
      ? "bg-amber-900/60 text-amber-300 border-amber-700"
      : "bg-slate-800 text-slate-400 border-slate-700";

  return (
    <div className={`rounded border px-3 py-2 text-xs font-mono ${color}`}>
      <div className="font-semibold">{label}</div>
      <div className="text-[10px] mt-0.5 opacity-60 truncate">{s.name}</div>
    </div>
  );
}

export default async function SnapshotPanel() {
  const data = await getSnapshots();

  return (
    <section className="space-y-3">
      <div className="flex items-end justify-between">
        <h2 className="text-lg font-semibold text-slate-200">APFS Snapshots</h2>
        <span className="text-xs text-slate-500">volume: {data.volume}</span>
      </div>
      {data.snapshots.length === 0 ? (
        <p className="text-slate-500 text-sm">No local snapshots found.</p>
      ) : (
        <div className="grid gap-2 sm:grid-cols-2">
          {data.snapshots.map((s) => (
            <SnapshotBadge key={s.name} s={s} />
          ))}
        </div>
      )}
      <p className="text-xs text-slate-600">
        Live from <code className="font-mono">oclnr snapshot audit</code>
      </p>
    </section>
  );
}
