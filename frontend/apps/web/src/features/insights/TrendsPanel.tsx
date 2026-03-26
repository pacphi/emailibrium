import {
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
import { useTemporalInsights } from './hooks/useInsights';

interface TrendsPanelProps {
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

function ChartCard({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="rounded-xl border border-gray-200 bg-white p-6 shadow-sm dark:border-gray-700 dark:bg-gray-800">
      <h3 className="mb-4 text-sm font-medium text-gray-500 dark:text-gray-400">{title}</h3>
      {children}
    </div>
  );
}

function PanelSkeleton() {
  return (
    <div className="animate-pulse space-y-6">
      {Array.from({ length: 4 }).map((_, i) => (
        <div key={i} className="h-72 rounded-xl bg-gray-200 dark:bg-gray-700" />
      ))}
    </div>
  );
}

export function TrendsPanel({ report, isLoading }: TrendsPanelProps) {
  const { data: temporal, isLoading: temporalLoading } = useTemporalInsights();

  const categories = Object.keys(report?.categoryBreakdown ?? {});

  const dayNames = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];

  const volumeData = temporal?.dailyVolume.map(d => ({
    date: d.date.slice(5),
    volume: d.count,
  })) ?? [];

  const dayData = temporal?.dayOfWeek.map(d => ({
    day: dayNames[d.day] ?? `Day ${d.day}`,
    count: d.count,
  })) ?? [];

  const hourData = temporal?.hourOfDay.map(d => ({
    hour: `${String(d.hour).padStart(2, '0')}:00`,
    count: d.count,
  })) ?? [];

  const categoryData = temporal?.dailyVolume.map(d => {
    const dayCategories = temporal.categoryDaily.filter(cd => cd.date === d.date);
    const row: Record<string, string | number> = { date: d.date.slice(5) };
    for (const cat of categories) {
      row[cat] = dayCategories.find(cd => cd.category === cat)?.count ?? 0;
    }
    return row;
  }) ?? [];

  if (isLoading || temporalLoading) return <PanelSkeleton />;

  return (
    <div className="space-y-6">
      {/* Volume over 90 days */}
      <ChartCard title="Email Volume Over Time (90 days)">
        <ResponsiveContainer width="100%" height={280}>
          <LineChart data={volumeData}>
            <XAxis dataKey="date" tick={{ fontSize: 10 }} interval={13} />
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
              dataKey="volume"
              stroke="#6366f1"
              strokeWidth={2}
              dot={false}
              name="Volume"
            />
          </LineChart>
        </ResponsiveContainer>
      </ChartCard>

      {/* Category distribution over time (stacked area via stacked bars) */}
      <ChartCard title="Category Distribution Over Time">
        <ResponsiveContainer width="100%" height={280}>
          <BarChart data={categoryData}>
            <XAxis dataKey="date" tick={{ fontSize: 10 }} interval={13} />
            <YAxis tick={{ fontSize: 11 }} width={36} />
            <Tooltip
              contentStyle={{
                border: '1px solid #e5e7eb',
                borderRadius: '0.5rem',
                fontSize: '0.875rem',
              }}
            />
            {categories.map((cat) => {
              const capCat = cat.charAt(0).toUpperCase() + cat.slice(1);
              return (
                <Bar
                  key={cat}
                  dataKey={cat}
                  stackId="categories"
                  fill={CATEGORY_COLORS[capCat] ?? '#94a3b8'}
                  name={capCat}
                />
              );
            })}
          </BarChart>
        </ResponsiveContainer>
        {/* Legend */}
        <div className="mt-2 flex flex-wrap gap-3">
          {categories.map((cat) => {
            const capCat = cat.charAt(0).toUpperCase() + cat.slice(1);
            return (
              <span key={cat} className="flex items-center gap-1.5 text-xs">
                <span
                  className="inline-block h-2.5 w-2.5 rounded-full"
                  style={{ backgroundColor: CATEGORY_COLORS[capCat] ?? '#94a3b8' }}
                />
                <span className="text-gray-600 dark:text-gray-300">{capCat}</span>
              </span>
            );
          })}
        </div>
      </ChartCard>

      {/* Day of week + Hour of day */}
      <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
        <ChartCard title="Busiest Day of Week">
          <ResponsiveContainer width="100%" height={240}>
            <BarChart data={dayData}>
              <XAxis dataKey="day" tick={{ fontSize: 11 }} />
              <YAxis tick={{ fontSize: 11 }} width={36} />
              <Tooltip
                contentStyle={{
                  border: '1px solid #e5e7eb',
                  borderRadius: '0.5rem',
                  fontSize: '0.875rem',
                }}
              />
              <Bar dataKey="count" fill="#6366f1" radius={[4, 4, 0, 0]} name="Emails" />
            </BarChart>
          </ResponsiveContainer>
        </ChartCard>

        <ChartCard title="Busiest Hour of Day">
          <ResponsiveContainer width="100%" height={240}>
            <BarChart data={hourData}>
              <XAxis dataKey="hour" tick={{ fontSize: 9 }} interval={2} />
              <YAxis tick={{ fontSize: 11 }} width={36} />
              <Tooltip
                contentStyle={{
                  border: '1px solid #e5e7eb',
                  borderRadius: '0.5rem',
                  fontSize: '0.875rem',
                }}
              />
              <Bar dataKey="count" fill="#8b5cf6" radius={[4, 4, 0, 0]} name="Emails" />
            </BarChart>
          </ResponsiveContainer>
        </ChartCard>
      </div>
    </div>
  );
}
