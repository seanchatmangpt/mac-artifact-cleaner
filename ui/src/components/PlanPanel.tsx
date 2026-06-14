import { getPlan } from "@/data/oclnr";

const KIND_COLOR: Record<string, string> = {
  dir: "bg-blue-900/50 text-blue-300",
  file: "bg-slate-800 text-slate-300",
};

export default async function PlanPanel() {
  const data = await getPlan();

  if (!data.found) {
    return (
      <section className="space-y-2">
        <h2 className="text-lg font-semibold text-slate-200">Deletion Plan</h2>
        <p className="text-slate-500 text-sm">
          No <code className="font-mono text-xs">cleanup-plan.json</code> found. Run{" "}
          <code className="font-mono text-xs">oclnr plan build</code> to create one.
        </p>
      </section>
    );
  }

  return (
    <section className="space-y-3">
      <div className="flex items-end justify-between">
        <h2 className="text-lg font-semibold text-slate-200">Deletion Plan</h2>
        <span className="text-xs text-slate-500 tabular-nums">{data.total_items} items</span>
      </div>

      {/* Metadata */}
      <div className="grid grid-cols-2 gap-3 rounded border border-slate-700 bg-slate-800/50 p-3 text-xs">
        <div>
          <div className="text-slate-500 mb-0.5">Created</div>
          <div className="text-slate-300 font-mono">{data.created}</div>
        </div>
        <div>
          <div className="text-slate-500 mb-0.5">Roots</div>
          <div className="text-slate-300 font-mono truncate">{data.roots.join(", ")}</div>
        </div>
        <div>
          <div className="text-slate-500 mb-0.5">Flags</div>
          <div className="flex gap-1.5">
            {data.flags.deps && (
              <span className="rounded bg-blue-900/60 border border-blue-700 px-1.5 py-0.5 text-blue-300 text-[10px]">deps</span>
            )}
            {data.flags.aggressive && (
              <span className="rounded bg-orange-900/60 border border-orange-700 px-1.5 py-0.5 text-orange-300 text-[10px]">aggressive</span>
            )}
          </div>
        </div>
        <div>
          <div className="text-slate-500 mb-0.5">Version</div>
          <div className="text-slate-300 font-mono">v{data.version}</div>
        </div>
      </div>

      {/* Top items */}
      <div className="space-y-1">
        <p className="text-xs text-slate-500 uppercase tracking-wide">First {data.items.length} of {data.total_items} scheduled deletions</p>
        <div className="rounded border border-slate-700 divide-y divide-slate-800 max-h-52 overflow-y-auto">
          {data.items.map((item, i) => (
            <div key={i} className="flex items-center gap-2 px-3 py-1.5 hover:bg-slate-800/40">
              <span className={`text-[10px] rounded px-1.5 py-0.5 font-mono font-semibold ${KIND_COLOR[item.kind] ?? "bg-slate-700 text-slate-300"}`}>
                {item.kind}
              </span>
              <span className="text-xs font-mono text-slate-300 truncate flex-1 min-w-0">{item.path}</span>
              <span className="text-[10px] text-slate-500 shrink-0">{item.reason}</span>
            </div>
          ))}
        </div>
      </div>

      <p className="text-xs text-slate-600">
        Read from <code className="font-mono">cleanup-plan.json</code> — created by{" "}
        <code className="font-mono">oclnr plan build</code>
      </p>
    </section>
  );
}
