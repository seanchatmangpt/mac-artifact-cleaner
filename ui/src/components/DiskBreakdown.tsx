import { getBreakdown } from "@/data/oclnr";
import type { BreakdownEntry } from "@/data/oclnr";

const CAT_COLOR: Record<BreakdownEntry["category"], string> = {
  "tool cache": "bg-yellow-400",
  "build artifact": "bg-red-400",
  "data": "bg-green-400",
  "project/other": "bg-slate-400",
};

const CAT_LABEL: Record<BreakdownEntry["category"], string> = {
  "tool cache": "Tool Cache",
  "build artifact": "Build Artifact",
  "data": "Data",
  "project/other": "Project",
};

export default async function DiskBreakdown() {
  const data = await getBreakdown();
  const usedGb = data.disk_total_gb - data.disk_free_gb;

  return (
    <section className="space-y-4">
      <div className="flex items-end justify-between">
        <h2 className="text-lg font-semibold text-slate-200">Disk Usage</h2>
        <span className="text-xs text-slate-500 tabular-nums">
          {data.disk_free_gb.toFixed(0)} GB free of {data.disk_total_gb.toFixed(0)} GB
        </span>
      </div>

      <div className="h-3 w-full rounded-full bg-slate-700 overflow-hidden">
        <div
          className="h-full rounded-full bg-blue-500 transition-all"
          style={{ width: `${data.disk_used_pct}%` }}
        />
      </div>
      <p className="text-xs text-slate-400 tabular-nums">
        {usedGb.toFixed(0)} GB used · {data.disk_used_pct}%
      </p>

      <div className="overflow-x-auto">
        <table className="w-full text-sm">
          <thead>
            <tr className="border-b border-slate-700 text-slate-400 text-xs uppercase tracking-wide">
              <th className="py-2 text-left font-medium">Path</th>
              <th className="py-2 text-right font-medium w-24">Size</th>
              <th className="py-2 text-right font-medium w-16">%</th>
              <th className="py-2 text-left font-medium w-32 pl-4">Category</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-slate-800">
            {data.entries.map((e) => (
              <tr key={e.path} className="hover:bg-slate-800/40 transition-colors">
                <td className="py-1.5 font-mono text-slate-300 text-xs">{e.path}</td>
                <td className="py-1.5 text-right tabular-nums text-slate-300 text-xs">{e.size_human}</td>
                <td className="py-1.5 text-right tabular-nums text-slate-400 text-xs">{e.pct}%</td>
                <td className="py-1.5 pl-4">
                  <span className="flex items-center gap-1.5">
                    <span className={`inline-block w-2 h-2 rounded-full ${CAT_COLOR[e.category]}`} />
                    <span className="text-slate-400 text-xs">{CAT_LABEL[e.category]}</span>
                  </span>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <p className="text-xs text-slate-600 pt-1">
        Scanned {data.total_scanned_human} — live output from{" "}
        <code className="font-mono">oclnr audit breakdown</code>
      </p>
    </section>
  );
}
