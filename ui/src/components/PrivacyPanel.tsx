import { getPrivacy } from "@/data/oclnr";

export default async function PrivacyPanel() {
  const data = await getPrivacy();

  return (
    <section className="space-y-3">
      <div className="flex items-end justify-between">
        <h2 className="text-lg font-semibold text-slate-200">Privacy Gate</h2>
        <span className={`text-xs font-semibold ${data.passed ? "text-green-400" : "text-red-400"}`}>
          {data.passed ? "Clean" : "Violations found"}
        </span>
      </div>

      {/* gitignore */}
      <div className={`flex items-center gap-2 rounded border px-3 py-2 text-xs ${
        data.gitignore_ok
          ? "bg-green-950/40 border-green-800 text-green-300"
          : "bg-red-950/40 border-red-800 text-red-300"
      }`}>
        <span>{data.gitignore_ok ? "✅" : "❌"}</span>
        <span>.gitignore patterns {data.gitignore_ok ? "complete" : "missing required patterns"}</span>
      </div>

      {/* Sensitive files */}
      {data.sensitive_files.length > 0 && (
        <div className="rounded border border-amber-800 bg-amber-950/30 p-3 space-y-1">
          <p className="text-xs font-semibold text-amber-400">
            ⚠️ Sensitive files in workspace ({data.sensitive_files.length})
          </p>
          {data.sensitive_files.map((f, i) => (
            <p key={i} className="text-[11px] font-mono text-slate-400">{f}</p>
          ))}
        </div>
      )}

      {/* Unredacted paths */}
      {data.unredacted_paths.length > 0 && (
        <details className="group">
          <summary className="cursor-pointer text-xs text-red-400 hover:text-red-300 select-none">
            {data.unredacted_paths.length} unredacted home paths ▸
          </summary>
          <div className="mt-2 rounded border border-slate-700 bg-slate-800/40 divide-y divide-slate-700 max-h-48 overflow-y-auto">
            {data.unredacted_paths.map((v, i) => (
              <div key={i} className="px-3 py-1.5 text-[11px] font-mono">
                <span className="text-slate-500">{v.file}:{v.line}</span>
                <span className="ml-2 text-slate-400">→ {v.content}</span>
              </div>
            ))}
          </div>
        </details>
      )}

      {data.sensitive_files.length === 0 && data.unredacted_paths.length === 0 && (
        <p className="text-xs text-green-400">No sensitive leaks detected.</p>
      )}

      <p className="text-xs text-slate-600">
        Live from <code className="font-mono">oclnr doctor privacy</code>
      </p>
    </section>
  );
}
