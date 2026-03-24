import {
  useSettings,
  type Theme,
  type SidebarPosition,
  type EmailListDensity,
} from './hooks/useSettings';

const THEME_OPTIONS: { value: Theme; label: string; description: string }[] = [
  { value: 'light', label: 'Light', description: 'Always use light mode' },
  { value: 'dark', label: 'Dark', description: 'Always use dark mode' },
  { value: 'system', label: 'System', description: 'Follow your OS preference' },
];

const SIDEBAR_OPTIONS: { value: SidebarPosition; label: string }[] = [
  { value: 'left', label: 'Left' },
  { value: 'right', label: 'Right' },
];

const DENSITY_OPTIONS: { value: EmailListDensity; label: string; description: string }[] = [
  { value: 'compact', label: 'Compact', description: 'More emails visible' },
  { value: 'comfortable', label: 'Comfortable', description: 'Balanced spacing' },
  { value: 'spacious', label: 'Spacious', description: 'More breathing room' },
];

export function AppearanceSettings() {
  const {
    theme,
    sidebarPosition,
    emailListDensity,
    fontSize,
    setTheme,
    setSidebarPosition,
    setEmailListDensity,
    setFontSize,
  } = useSettings();

  return (
    <div className="space-y-6">
      <h3 className="text-base font-semibold text-gray-900 dark:text-gray-100">Appearance</h3>

      {/* Theme */}
      <fieldset className="space-y-2">
        <legend className="text-sm font-medium text-gray-700 dark:text-gray-300">Theme</legend>
        <div className="flex gap-3">
          {THEME_OPTIONS.map((option) => {
            const isSelected = theme === option.value;
            return (
              <label
                key={option.value}
                className={`flex-1 flex flex-col items-center gap-1 p-3 rounded-lg border-2 cursor-pointer
                  transition-all text-center ${
                    isSelected
                      ? 'border-indigo-500 bg-indigo-50 dark:bg-indigo-900/20 dark:border-indigo-400'
                      : 'border-gray-200 bg-white hover:border-gray-300 dark:bg-gray-800 dark:border-gray-700'
                  }`}
              >
                <input
                  type="radio"
                  name="theme"
                  value={option.value}
                  checked={isSelected}
                  onChange={() => setTheme(option.value)}
                  className="sr-only"
                />
                <span className="text-sm font-medium text-gray-900 dark:text-gray-100">
                  {option.label}
                </span>
                <span className="text-xs text-gray-500 dark:text-gray-400">
                  {option.description}
                </span>
              </label>
            );
          })}
        </div>
      </fieldset>

      {/* Sidebar position */}
      <div className="space-y-2">
        <span className="block text-sm font-medium text-gray-700 dark:text-gray-300">
          Sidebar Position
        </span>
        <div className="flex gap-3 max-w-xs">
          {SIDEBAR_OPTIONS.map((option) => {
            const isSelected = sidebarPosition === option.value;
            return (
              <button
                key={option.value}
                type="button"
                onClick={() => setSidebarPosition(option.value)}
                className={`flex-1 px-4 py-2 rounded-lg border-2 text-sm font-medium transition-all ${
                  isSelected
                    ? 'border-indigo-500 bg-indigo-50 text-indigo-700 dark:bg-indigo-900/20 dark:border-indigo-400 dark:text-indigo-300'
                    : 'border-gray-200 bg-white text-gray-700 hover:border-gray-300 dark:bg-gray-800 dark:border-gray-700 dark:text-gray-300'
                }`}
              >
                {option.label}
              </button>
            );
          })}
        </div>
      </div>

      {/* Email list density */}
      <fieldset className="space-y-2">
        <legend className="text-sm font-medium text-gray-700 dark:text-gray-300">
          Email List Density
        </legend>
        <div className="flex gap-3">
          {DENSITY_OPTIONS.map((option) => {
            const isSelected = emailListDensity === option.value;
            return (
              <label
                key={option.value}
                className={`flex-1 flex flex-col items-center gap-1 p-3 rounded-lg border-2 cursor-pointer
                  transition-all text-center ${
                    isSelected
                      ? 'border-indigo-500 bg-indigo-50 dark:bg-indigo-900/20 dark:border-indigo-400'
                      : 'border-gray-200 bg-white hover:border-gray-300 dark:bg-gray-800 dark:border-gray-700'
                  }`}
              >
                <input
                  type="radio"
                  name="density"
                  value={option.value}
                  checked={isSelected}
                  onChange={() => setEmailListDensity(option.value)}
                  className="sr-only"
                />
                <span className="text-sm font-medium text-gray-900 dark:text-gray-100">
                  {option.label}
                </span>
                <span className="text-xs text-gray-500 dark:text-gray-400">
                  {option.description}
                </span>
              </label>
            );
          })}
        </div>
      </fieldset>

      {/* Font size */}
      <div className="space-y-1 max-w-sm">
        <label
          htmlFor="font-size"
          className="block text-sm font-medium text-gray-700 dark:text-gray-300"
        >
          Font Size
        </label>
        <div className="flex items-center gap-3">
          <span className="text-xs text-gray-500 dark:text-gray-400">A</span>
          <input
            id="font-size"
            type="range"
            min={12}
            max={20}
            step={1}
            value={fontSize}
            onChange={(e) => setFontSize(Number(e.target.value))}
            className="flex-1 h-2 bg-gray-200 rounded-lg appearance-none cursor-pointer
              dark:bg-gray-700 accent-indigo-600"
          />
          <span className="text-base text-gray-500 dark:text-gray-400 font-medium">A</span>
          <span className="text-sm text-gray-600 dark:text-gray-400 w-10 text-right tabular-nums">
            {fontSize}px
          </span>
        </div>
        <p className="text-gray-500 dark:text-gray-400 mt-2" style={{ fontSize: `${fontSize}px` }}>
          Preview text at {fontSize}px
        </p>
      </div>
    </div>
  );
}
