import { useMemo } from 'react';
import {
  LineChart,
  Line,
  BarChart,
  Bar,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
} from 'recharts';
import type { Rule } from '@emailibrium/types';

interface RuleMetricsProps {
  rules: Rule[];
}

interface AccuracyPoint {
  name: string;
  accuracy: number;
}

interface MatchPoint {
  name: string;
  matches: number;
}

export function RuleMetrics({ rules }: RuleMetricsProps) {
  const accuracyData: AccuracyPoint[] = useMemo(
    () =>
      rules.map((r) => ({
        name: r.name.length > 20 ? r.name.slice(0, 20) + '...' : r.name,
        accuracy: Math.round(r.accuracy * 100),
      })),
    [rules],
  );

  const matchData: MatchPoint[] = useMemo(
    () =>
      rules
        .slice()
        .sort((a, b) => b.matchCount - a.matchCount)
        .slice(0, 10)
        .map((r) => ({
          name: r.name.length > 15 ? r.name.slice(0, 15) + '...' : r.name,
          matches: r.matchCount,
        })),
    [rules],
  );

  const avgAccuracy = useMemo(() => {
    if (rules.length === 0) return 0;
    return Math.round((rules.reduce((sum, r) => sum + r.accuracy, 0) / rules.length) * 100);
  }, [rules]);

  const totalMatches = useMemo(() => rules.reduce((sum, r) => sum + r.matchCount, 0), [rules]);

  if (rules.length === 0) {
    return (
      <p className="py-8 text-center text-sm text-gray-500 dark:text-gray-400">
        No rules to display metrics for.
      </p>
    );
  }

  return (
    <div className="space-y-6">
      {/* Summary cards */}
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-3">
        <MetricCard label="Total Rules" value={rules.length.toString()} />
        <MetricCard label="Average Accuracy" value={`${avgAccuracy}%`} />
        <MetricCard label="Total Matches" value={totalMatches.toLocaleString()} />
      </div>

      {/* Accuracy chart */}
      <div className="rounded-lg border border-gray-200 bg-white p-4 dark:border-gray-700 dark:bg-gray-800">
        <h4 className="mb-3 text-sm font-semibold text-gray-900 dark:text-white">
          Accuracy per Rule
        </h4>
        <div className="h-64">
          <ResponsiveContainer width="100%" height="100%">
            <LineChart data={accuracyData}>
              <CartesianGrid strokeDasharray="3 3" stroke="#374151" opacity={0.2} />
              <XAxis
                dataKey="name"
                tick={{ fontSize: 11 }}
                angle={-30}
                textAnchor="end"
                height={60}
              />
              <YAxis domain={[0, 100]} tick={{ fontSize: 11 }} tickFormatter={(v) => `${v}%`} />
              <Tooltip formatter={(val: number) => `${val}%`} />
              <Line
                type="monotone"
                dataKey="accuracy"
                stroke="#6366f1"
                strokeWidth={2}
                dot={{ r: 4 }}
                activeDot={{ r: 6 }}
              />
            </LineChart>
          </ResponsiveContainer>
        </div>
      </div>

      {/* Matches bar chart */}
      <div className="rounded-lg border border-gray-200 bg-white p-4 dark:border-gray-700 dark:bg-gray-800">
        <h4 className="mb-3 text-sm font-semibold text-gray-900 dark:text-white">
          Total Matches by Rule (Top 10)
        </h4>
        <div className="h-64">
          <ResponsiveContainer width="100%" height="100%">
            <BarChart data={matchData}>
              <CartesianGrid strokeDasharray="3 3" stroke="#374151" opacity={0.2} />
              <XAxis
                dataKey="name"
                tick={{ fontSize: 11 }}
                angle={-30}
                textAnchor="end"
                height={60}
              />
              <YAxis tick={{ fontSize: 11 }} />
              <Tooltip />
              <Bar dataKey="matches" fill="#6366f1" radius={[4, 4, 0, 0]} />
            </BarChart>
          </ResponsiveContainer>
        </div>
      </div>
    </div>
  );
}

function MetricCard({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg border border-gray-200 bg-white p-4 dark:border-gray-700 dark:bg-gray-800">
      <p className="text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400">
        {label}
      </p>
      <p className="mt-1 text-2xl font-bold text-gray-900 dark:text-white">{value}</p>
    </div>
  );
}
