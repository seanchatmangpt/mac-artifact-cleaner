import { getDoctor } from "@/data/oclnr";

export default async function DoctorPanel() {
  const data = await getDoctor();

  return (
    <section className="space-y-3">
      <div className="flex items-end justify-between">
        <h2 className="text-lg font-semibold text-slate-200">Doctor</h2>
        <span className={`text-xs font-semibold ${data.all_passed ? "text-green-400" : "text-red-400"}`}>
          {data.all_passed ? "All checks passed" : "Issues detected"}
        </span>
      </div>
      <div className="space-y-2">
        {data.checks.map((c) => (
          <div
            key={c.name}
            className={`rounded border p-3 ${
              c.passed ? "bg-green-950/40 border-green-800" : "bg-red-950/40 border-red-800"
            }`}
          >
            <div className="flex items-center gap-2 mb-1">
              <span className={`text-sm ${c.passed ? "text-green-400" : "text-red-400"}`}>
                {c.passed ? "✅" : "❌"}
              </span>
              <span className="text-sm font-medium text-slate-200">{c.name}</span>
            </div>
            <pre className="text-[11px] text-slate-400 whitespace-pre-wrap font-mono leading-relaxed overflow-x-auto max-h-40">
              {c.output}
            </pre>
          </div>
        ))}
      </div>
      <p className="text-xs text-slate-600">
        Live from <code className="font-mono">oclnr doctor architecture</code> and{" "}
        <code className="font-mono">oclnr doctor substrate</code>
      </p>
    </section>
  );
}
