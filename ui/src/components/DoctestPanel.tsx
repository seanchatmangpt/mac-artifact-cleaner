import { getDoctests } from "@/data/oclnr";

export default async function DoctestPanel() {
  const data = await getDoctests();

  return (
    <section className="space-y-3">
      <div className="flex items-end justify-between">
        <h2 className="text-lg font-semibold text-slate-200">Doctest Coverage</h2>
        <span className={`text-xs font-semibold ${data.passed ? "text-green-400" : "text-amber-400"}`}>
          {data.functions_checked} functions · {data.missing_doctests} missing
        </span>
      </div>

      {/* Module doc coverage */}
      <div className="grid grid-cols-2 sm:grid-cols-3 gap-1.5">
        {data.modules.map((m) => (
          <div
            key={m.file}
            className={`flex items-center gap-1.5 rounded px-2 py-1 text-xs font-mono ${
              m.has_module_doc
                ? "bg-green-950/40 text-green-300 border border-green-900"
                : "bg-slate-800/60 text-slate-400 border border-slate-700"
            }`}
          >
            <span>{m.has_module_doc ? "✓" : "–"}</span>
            <span className="truncate">{m.file}</span>
          </div>
        ))}
      </div>

      {/* Missing function list */}
      {data.missing_functions.length > 0 && (
        <details className="group">
          <summary className="cursor-pointer text-xs text-amber-400 hover:text-amber-300 select-none">
            {data.missing_functions.length} functions missing doctests ▸
          </summary>
          <ul className="mt-2 space-y-0.5 pl-3 border-l border-slate-700">
            {data.missing_functions.map((f, i) => (
              <li key={i} className="text-[11px] font-mono text-slate-400">{f}</li>
            ))}
          </ul>
        </details>
      )}

      <p className="text-xs text-slate-600">
        Live from <code className="font-mono">oclnr doctor doctests</code>
      </p>
    </section>
  );
}
