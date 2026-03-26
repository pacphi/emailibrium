import { useState } from 'react';
import {
  PieChart,
  Pie,
  Cell,
  LineChart,
  Line,
  BarChart,
  Bar,
  XAxis,
  YAxis,
  Tooltip,
  ResponsiveContainer,
} from 'recharts';
import type { InboxReport } from '@emailibrium/types';
import { HealthScoreGauge } from './components/HealthScoreGauge';
import { useTemporalInsights } from './hooks/useInsights';

interface OverviewPanelProps {
  report: InboxReport | undefined;
  isLoading: boolean;
}

const CATEGORY_COLORS: Record<string, string> = {
  Work: '#6366f1',
  Personal: '#8b5cf6',
  Finance: '#10b981',
  Shopping: '#f59e0b',
  Social: '#ec4899',
  Newsletter: '#06b6d4',
  Marketing: '#f97316',
  Notifications: '#64748b',
};

const DEFAULT_CATEGORIES = [
  'Work',
  'Personal',
  'Finance',
  'Shopping',
  'Social',
  'Newsletter',
  'Marketing',
  'Notifications',
];

function buildCategoryData(breakdown: Record<string, number> | undefined) {
  if (!breakdown) return [];
  return DEFAULT_CATEGORIES.map((name) => ({
    name,
    value: breakdown[name.toLowerCase()] ?? breakdown[name] ?? 0,
  })).filter((d) => d.value > 0);
}

function computeHealthScore(report: InboxReport | undefined): number {
  if (!report) return 0;
  const { totalEmails, subscriptionCount, estimatedReadingHours } = report;
  if (totalEmails === 0) return 100;
  const subRatio = subscriptionCount / Math.max(totalEmails, 1);
  const readingPenalty = Math.min(estimatedReadingHours / 10, 1);
  return Math.round(Math.max(0, Math.min(100, 100 - subRatio * 60 - readingPenalty * 30)));
}

function PanelSkeleton() {
  return (
    <div className="animate-pulse space-y-6">
      <div className="grid grid-cols-1 gap-6 lg:grid-cols-3">
        <div className="flex items-center justify-center">
          <div className="h-40 w-40 rounded-full bg-gray-200 dark:bg-gray-700" />
        </div>
        <div className="h-64 rounded-lg bg-gray-200 dark:bg-gray-700 lg:col-span-2" />
      </div>
      <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
        <div className="h-64 rounded-lg bg-gray-200 dark:bg-gray-700" />
        <div className="h-64 rounded-lg bg-gray-200 dark:bg-gray-700" />
      </div>
    </div>
  );
}

export function OverviewPanel({ report, isLoading }: OverviewPanelProps) {
  const [selectedCategory, setSelectedCategory] = useState<string | null>(null);
  const { data: temporal } = useTemporalInsights();

  if (isLoading) return <PanelSkeleton />;

  const healthScore = computeHealthScore(report);
  const rawCategoryData = buildCategoryData(report?.categoryBreakdown);
  // Hide category breakdown when embeddings aren't active (all Uncategorized).
  const hasRealCategories = rawCategoryData.some(
    (d) => d.name !== 'Uncategorized' && d.name !== 'unknown',
  );
  const categoryData = hasRealCategories ? rawCategoryData : [];
  const volumeData =
    temporal?.dailyVolume.slice(-30).map((d) => ({
      date: d.date.slice(5),
      received: d.count,
    })) ?? [];
  const topSenders = (report?.topSenders ?? []).slice(0, 8).map((s) => {
    // Strip email address from "Display Name <email>" format, show just the name.
    const match = s.sender.match(/^"?(.+?)"?\s*<.+>$/);
    const name = match?.[1] ?? s.sender.replace(/<.*>/, '').trim();
    // Fall back to domain if name is empty
    const label = name || s.sender.split('@')[0] || s.sender;
    return { sender: label, count: s.count };
  });

  return (
    <div className="space-y-6">
      {/* Row 1: Health Score + Category Breakdown */}
      <div className="grid grid-cols-1 gap-6 lg:grid-cols-3">
        {/* Health Score */}
        <div className="flex flex-col items-center justify-center rounded-xl border border-gray-200 bg-white p-6 shadow-sm dark:border-gray-700 dark:bg-gray-800">
          <h3 className="mb-4 text-sm font-medium text-gray-500 dark:text-gray-400">
            Inbox Health Score
          </h3>
          <HealthScoreGauge score={healthScore} />
        </div>

        {/* Category Breakdown */}
        <div className="rounded-xl border border-gray-200 bg-white p-6 shadow-sm dark:border-gray-700 dark:bg-gray-800 lg:col-span-2">
          <h3 className="mb-4 text-sm font-medium text-gray-500 dark:text-gray-400">
            Category Breakdown
            {selectedCategory && (
              <button
                onClick={() => setSelectedCategory(null)}
                className="ml-2 text-xs text-indigo-600 hover:text-indigo-500 dark:text-indigo-400"
              >
                Clear filter
              </button>
            )}
          </h3>
          {categoryData.length === 0 ? (
            <p className="py-12 text-center text-sm text-gray-400">
              Enable embeddings to see category breakdown
            </p>
          ) : (
            <ResponsiveContainer width="100%" height={240}>
              <PieChart>
                <Pie
                  data={categoryData}
                  cx="50%"
                  cy="50%"
                  innerRadius={60}
                  outerRadius={100}
                  paddingAngle={2}
                  dataKey="value"
                  onClick={(entry) => setSelectedCategory(entry.name)}
                  className="cursor-pointer"
                >
                  {categoryData.map((entry) => (
                    <Cell
                      key={entry.name}
                      fill={CATEGORY_COLORS[entry.name] ?? '#94a3b8'}
                      opacity={selectedCategory && selectedCategory !== entry.name ? 0.3 : 1}
                    />
                  ))}
                </Pie>
                <Tooltip
                  contentStyle={{
                    backgroundColor: 'var(--tw-bg-opacity, #fff)',
                    border: '1px solid #e5e7eb',
                    borderRadius: '0.5rem',
                    fontSize: '0.875rem',
                  }}
                />
              </PieChart>
            </ResponsiveContainer>
          )}
          {/* Legend */}
          <div className="mt-2 flex flex-wrap gap-3">
            {categoryData.map((entry) => (
              <button
                key={entry.name}
                onClick={() =>
                  setSelectedCategory(selectedCategory === entry.name ? null : entry.name)
                }
                className={`flex items-center gap-1.5 text-xs ${
                  selectedCategory && selectedCategory !== entry.name ? 'opacity-40' : 'opacity-100'
                }`}
              >
                <span
                  className="inline-block h-2.5 w-2.5 rounded-full"
                  style={{ backgroundColor: CATEGORY_COLORS[entry.name] ?? '#94a3b8' }}
                />
                <span className="text-gray-600 dark:text-gray-300">
                  {entry.name} ({entry.value})
                </span>
              </button>
            ))}
          </div>
        </div>
      </div>

      {/* Row 2: Volume Chart + Top Senders */}
      <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
        {/* Email Volume */}
        <div className="rounded-xl border border-gray-200 bg-white p-6 shadow-sm dark:border-gray-700 dark:bg-gray-800">
          <h3 className="mb-4 text-sm font-medium text-gray-500 dark:text-gray-400">
            Email Volume (30 days)
          </h3>
          <ResponsiveContainer width="100%" height={240}>
            <LineChart data={volumeData}>
              <XAxis
                dataKey="date"
                tick={{ fontSize: 11 }}
                interval="preserveStartEnd"
                className="text-gray-500"
              />
              <YAxis tick={{ fontSize: 11 }} width={36} />
              <Tooltip
                contentStyle={{
                  border: '1px solid #e5e7eb',
                  borderRadius: '0.5rem',
                  fontSize: '0.875rem',
                }}
              />
              <Line
                type="monotone"
                dataKey="received"
                stroke="#6366f1"
                strokeWidth={2}
                dot={false}
                name="Received"
              />
            </LineChart>
          </ResponsiveContainer>
          <div className="mt-2 flex gap-4 text-xs text-gray-500 dark:text-gray-400">
            <span className="flex items-center gap-1.5">
              <span className="inline-block h-2 w-4 rounded bg-indigo-500" />
              Received
            </span>
          </div>
        </div>

        {/* Top Senders */}
        <div className="rounded-xl border border-gray-200 bg-white p-6 shadow-sm dark:border-gray-700 dark:bg-gray-800">
          <h3 className="mb-4 text-sm font-medium text-gray-500 dark:text-gray-400">Top Senders</h3>
          {topSenders.length === 0 ? (
            <p className="py-12 text-center text-sm text-gray-400">No sender data available</p>
          ) : (
            <ResponsiveContainer width="100%" height={280}>
              <BarChart data={topSenders} layout="vertical" margin={{ left: 10 }}>
                <XAxis type="number" tick={{ fontSize: 11 }} />
                <YAxis type="category" dataKey="sender" tick={{ fontSize: 12 }} width={160} />
                <Tooltip
                  contentStyle={{
                    border: '1px solid #e5e7eb',
                    borderRadius: '0.5rem',
                    fontSize: '0.875rem',
                  }}
                />
                <Bar dataKey="count" name="Emails" radius={[0, 4, 4, 0]}>
                  {topSenders.map((_, i) => (
                    <Cell
                      key={i}
                      fill={
                        Object.values(CATEGORY_COLORS)[i % Object.values(CATEGORY_COLORS).length]
                      }
                    />
                  ))}
                </Bar>
              </BarChart>
            </ResponsiveContainer>
          )}
        </div>
      </div>
    </div>
  );
}
