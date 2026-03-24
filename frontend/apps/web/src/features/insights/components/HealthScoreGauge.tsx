interface HealthScoreGaugeProps {
  score: number;
  size?: number;
  onImprove?: () => void;
}

function getScoreColor(score: number): string {
  if (score < 40) return '#ef4444';
  if (score <= 70) return '#eab308';
  return '#22c55e';
}

function getScoreLabel(score: number): string {
  if (score < 40) return 'Poor';
  if (score <= 70) return 'Good';
  return 'Excellent';
}

function getScoreBgClass(score: number): string {
  if (score < 40) return 'text-red-500';
  if (score <= 70) return 'text-yellow-500';
  return 'text-green-500';
}

export function HealthScoreGauge({ score, size = 160, onImprove }: HealthScoreGaugeProps) {
  const clamped = Math.max(0, Math.min(100, score));
  const strokeWidth = 12;
  const radius = (size - strokeWidth) / 2;
  const circumference = 2 * Math.PI * radius;
  const progress = (clamped / 100) * circumference;
  const center = size / 2;
  const color = getScoreColor(clamped);
  const label = getScoreLabel(clamped);

  return (
    <div className="flex flex-col items-center gap-2">
      <svg
        width={size}
        height={size}
        viewBox={`0 0 ${size} ${size}`}
        className="drop-shadow-sm"
        role="img"
        aria-label={`Inbox health score: ${clamped} out of 100 - ${label}`}
      >
        {/* Background track */}
        <circle
          cx={center}
          cy={center}
          r={radius}
          fill="none"
          stroke="currentColor"
          strokeWidth={strokeWidth}
          className="text-gray-200 dark:text-gray-700"
        />
        {/* Progress arc */}
        <circle
          cx={center}
          cy={center}
          r={radius}
          fill="none"
          stroke={color}
          strokeWidth={strokeWidth}
          strokeLinecap="round"
          strokeDasharray={circumference}
          strokeDashoffset={circumference - progress}
          transform={`rotate(-90 ${center} ${center})`}
          className="transition-all duration-700 ease-out"
        />
        {/* Score text */}
        <text
          x={center}
          y={center - 6}
          textAnchor="middle"
          dominantBaseline="central"
          className="fill-gray-900 text-3xl font-bold dark:fill-white"
          style={{ fontSize: size * 0.2 }}
        >
          {clamped}
        </text>
        <text
          x={center}
          y={center + size * 0.12}
          textAnchor="middle"
          dominantBaseline="central"
          className={`font-medium ${getScoreBgClass(clamped)}`}
          style={{ fontSize: size * 0.09 }}
          fill={color}
        >
          {label}
        </text>
      </svg>
      {onImprove && (
        <button
          onClick={onImprove}
          className="text-sm font-medium text-indigo-600 hover:text-indigo-500 dark:text-indigo-400 dark:hover:text-indigo-300"
        >
          Improve Score
        </button>
      )}
    </div>
  );
}
