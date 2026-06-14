import { Suspense } from "react";
import { findLargeFiles } from "@/data/oclnr";
import Link from "next/link";

async function FindLargeResults({ minMb }: { minMb: number }) {
  const data = await findLargeFiles(minMb, 60000);

  if (data.timed_out) {
    return (
      <div className="rounded border border-amber-800 bg-amber-950/30 p-4 text-sm text-amber-300">
        <p className="font-semibold">Scan timed out after 60s</p>
        <p className="text-xs mt-1 text-amber-400/70">
          Scanned {data.files_scanned.toLocaleString()} files. No files ≥{minMb} MB found before timeout.
          Try a larger threshold or run <code className="font-mono">oclnr audit find-large --min-mb {minMb}</code> directly.
        </p>
      </div>
    );
  }

  if (data.files.length === 0) {
    return (
      <div className="rounded border border-slate-700 bg-slate-800/40 p-4 text-sm text-slate-400">
        <p>No files ≥ {minMb} MB found.</p>
        <p className="text-xs mt-1 text-slate-500">
          Scanned {data.files_scanned.toLocaleString()} files.
        </p>
      </div>
    );
  }

  return (
    <div className="space-y-3">
      <p className="text-xs text-slate-500">
        {data.files.length} files ≥ {minMb} MB · scanned {data.files_scanned.toLocaleString()} total
      </p>
      <div className="rounded border border-slate-700 divide-y divide-slate-800 overflow-x-auto">
        {data.files.map((f, i) => (
          <div key={i} className="flex items-center gap-4 px-3 py-2 hover:bg-slate-800/40">
            <span className="text-sm font-bold tabular-nums text-slate-200 w-24 shrink-0 text-right">
              {f.size_human}
            </span>
            <span className="text-xs font-mono text-slate-400 truncate">{f.path}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

function ScanningIndicator({ minMb }: { minMb: number }) {
  return (
    <div className="space-y-3">
      <div className="flex items-center gap-3 text-sm text-slate-400">
        <div className="h-4 w-4 rounded-full border-2 border-blue-500 border-t-transparent animate-spin" />
        <span>Scanning for files ≥ {minMb} MB — this may take up to 60 seconds…</span>
      </div>
      <div className="animate-pulse space-y-2">
        {[80, 65, 72, 55, 68].map((w, i) => (
          <div key={i} className="h-8 rounded bg-slate-700" style={{ width: `${w}%` }} />
        ))}
      </div>
      <p className="text-xs text-slate-600">
        Running <code className="font-mono">oclnr audit find-large --min-mb {minMb}</code> — streaming result when complete
      </p>
    </div>
  );
}

export default async function FindLargePage({
  searchParams,
}: {
  searchParams: Promise<{ min_mb?: string }>;
}) {
  const params = await searchParams;
  const minMb = Math.max(10, parseInt(params.min_mb ?? "100") || 100);

  return (
    <div className="min-h-screen bg-slate-950 text-slate-100">
      <header className="border-b border-slate-800 px-6 py-4">
        <div className="max-w-4xl mx-auto flex items-center gap-4">
          <Link href="/" className="text-slate-500 hover:text-slate-300 text-sm">← oclnr</Link>
          <h1 className="text-xl font-bold tracking-tight">Large File Scan</h1>
        </div>
      </header>

      <main className="max-w-4xl mx-auto px-6 py-8 space-y-6">
        {/* Threshold selector */}
        <div className="rounded-xl border border-slate-800 bg-slate-900 p-6 space-y-4">
          <div className="flex items-center justify-between">
            <h2 className="text-base font-semibold text-slate-200">
              Threshold: ≥ {minMb} MB
            </h2>
            <div className="flex gap-2">
              {[100, 500, 1000, 2000].map((mb) => (
                <Link
                  key={mb}
                  href={`/find-large?min_mb=${mb}`}
                  className={`px-3 py-1 rounded text-xs font-medium transition-colors ${
                    minMb === mb
                      ? "bg-blue-600 text-white"
                      : "bg-slate-800 text-slate-400 hover:bg-slate-700"
                  }`}
                >
                  {mb >= 1000 ? `${mb / 1000}GB` : `${mb}MB`}+
                </Link>
              ))}
            </div>
          </div>

          <Suspense fallback={<ScanningIndicator minMb={minMb} />}>
            <FindLargeResults minMb={minMb} />
          </Suspense>
        </div>

        <p className="text-xs text-slate-600 text-center">
          Real output from <code className="font-mono">oclnr audit find-large --min-mb {minMb} --top 20</code>
        </p>
      </main>
    </div>
  );
}
