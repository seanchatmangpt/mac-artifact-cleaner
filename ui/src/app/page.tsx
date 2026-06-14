import { Suspense } from "react";
import DiskBreakdown from "@/components/DiskBreakdown";
import SnapshotPanel from "@/components/SnapshotPanel";
import DoctorPanel from "@/components/DoctorPanel";
import ReceiptPanel from "@/components/ReceiptPanel";

function Skeleton({ rows = 4 }: { rows?: number }) {
  return (
    <div className="animate-pulse space-y-2">
      {Array.from({ length: rows }).map((_, i) => (
        <div
          key={i}
          className="h-4 rounded bg-slate-700"
          style={{ width: `${70 + (i % 3) * 10}%` }}
        />
      ))}
    </div>
  );
}

function Card({ children }: { children: React.ReactNode }) {
  return (
    <div className="rounded-xl border border-slate-800 bg-slate-900 p-6">
      {children}
    </div>
  );
}

export default function Home() {
  return (
    <div className="min-h-screen bg-slate-950 text-slate-100">
      <header className="border-b border-slate-800 px-6 py-4">
        <div className="max-w-6xl mx-auto flex items-center justify-between">
          <div>
            <h1 className="text-xl font-bold tracking-tight">oclnr</h1>
            <p className="text-xs text-slate-500 mt-0.5">
              macOS developer disk auditor · all data live from binary
            </p>
          </div>
          <code className="text-xs text-slate-600 font-mono">pentecost</code>
        </div>
      </header>

      <main className="max-w-6xl mx-auto px-6 py-8 space-y-6">
        <Card>
          <Suspense fallback={<Skeleton rows={8} />}>
            <DiskBreakdown />
          </Suspense>
        </Card>

        <div className="grid gap-6 lg:grid-cols-2">
          <Card>
            <Suspense fallback={<Skeleton rows={4} />}>
              <SnapshotPanel />
            </Suspense>
          </Card>
          <Card>
            <Suspense fallback={<Skeleton rows={4} />}>
              <ReceiptPanel />
            </Suspense>
          </Card>
        </div>

        <Card>
          <Suspense fallback={<Skeleton rows={6} />}>
            <DoctorPanel />
          </Suspense>
        </Card>
      </main>

      <footer className="border-t border-slate-800 px-6 py-4 text-center">
        <p className="text-xs text-slate-600">
          Faithful representation only — no fixtures, no mocks. Every number from{" "}
          <code className="font-mono">target/release/oclnr</code>.
        </p>
      </footer>
    </div>
  );
}
