import type { Provider } from '@emailibrium/types';

interface ProviderOption {
  id: Provider | 'other';
  name: string;
  description: string;
  icon: React.ReactNode;
}

const PROVIDERS: ProviderOption[] = [
  {
    id: 'gmail',
    name: 'Gmail',
    description: 'Connect your Google account via OAuth',
    icon: (
      <svg viewBox="0 0 24 24" className="w-8 h-8" aria-hidden="true">
        <path
          d="M24 5.457v13.909c0 .904-.732 1.636-1.636 1.636h-3.819V11.73L12 16.64l-6.545-4.91v9.273H1.636A1.636 1.636 0 010 19.366V5.457c0-2.023 2.309-3.178 3.927-1.964L12 9.545l8.073-6.052C21.691 2.279 24 3.434 24 5.457z"
          fill="#EA4335"
        />
      </svg>
    ),
  },
  {
    id: 'outlook',
    name: 'Outlook',
    description: 'Connect your Microsoft account via OAuth',
    icon: (
      <svg viewBox="0 0 24 24" className="w-8 h-8" aria-hidden="true">
        <path
          d="M24 7.387v10.478c0 .23-.08.424-.238.576a.806.806 0 01-.587.234h-8.55v-6.9l1.675 1.238a.39.39 0 00.462 0l6.975-5.088a.272.272 0 01.263.012.244.244 0 01.137.213zm0-1.588c0-.4-.288-.587-.863-.563l-7.512 5.475-1.675-1.238H9.375v13.15h13.8c.225 0 .413-.075.563-.225s.225-.338.225-.563V10.05L24 7.387zM14.625.6H1.2C.537.6 0 1.137 0 1.8v20.4c0 .663.537 1.2 1.2 1.2h13.425V.6zm-2.7 16.275c-.55.788-1.3 1.2-2.25 1.238-.9-.038-1.613-.45-2.138-1.238-.525-.787-.787-1.762-.787-2.925 0-1.2.275-2.2.825-3 .55-.8 1.275-1.213 2.175-1.238.9.025 1.625.438 2.175 1.238.55.8.825 1.8.825 3 0 1.163-.275 2.137-.825 2.925z"
          fill="#0078D4"
        />
      </svg>
    ),
  },
  {
    id: 'imap',
    name: 'IMAP',
    description: 'Yahoo, iCloud, Fastmail, and more',
    icon: (
      <svg
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth={1.5}
        className="w-8 h-8 text-gray-600"
        aria-hidden="true"
      >
        <path
          strokeLinecap="round"
          strokeLinejoin="round"
          d="M21.75 6.75v10.5a2.25 2.25 0 01-2.25 2.25h-15a2.25 2.25 0 01-2.25-2.25V6.75m19.5 0A2.25 2.25 0 0019.5 4.5h-15a2.25 2.25 0 00-2.25 2.25m19.5 0v.243a2.25 2.25 0 01-1.07 1.916l-7.5 4.615a2.25 2.25 0 01-2.36 0L3.32 8.91a2.25 2.25 0 01-1.07-1.916V6.75"
        />
      </svg>
    ),
  },
  {
    id: 'other',
    name: 'Other',
    description: 'Generic IMAP/SMTP configuration',
    icon: (
      <svg
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth={1.5}
        className="w-8 h-8 text-gray-400"
        aria-hidden="true"
      >
        <path
          strokeLinecap="round"
          strokeLinejoin="round"
          d="M12 21a9.004 9.004 0 008.716-6.747M12 21a9.004 9.004 0 01-8.716-6.747M12 21c2.485 0 4.5-4.03 4.5-9S14.485 3 12 3m0 18c-2.485 0-4.5-4.03-4.5-9S9.515 3 12 3m0 0a8.997 8.997 0 017.843 4.582M12 3a8.997 8.997 0 00-7.843 4.582m15.686 0A11.953 11.953 0 0112 10.5c-2.998 0-5.74-1.1-7.843-2.918m15.686 0A8.959 8.959 0 0121 12c0 .778-.099 1.533-.284 2.253m0 0A17.919 17.919 0 0112 16.5c-3.162 0-6.133-.815-8.716-2.247m0 0A9.015 9.015 0 013 12c0-1.605.42-3.113 1.157-4.418"
        />
      </svg>
    ),
  },
];

interface ProviderSelectorProps {
  onSelect: (provider: Provider | 'other') => void;
}

export function ProviderSelector({ onSelect }: ProviderSelectorProps) {
  return (
    <div className="grid grid-cols-2 gap-4 max-w-lg mx-auto">
      {PROVIDERS.map((provider) => (
        <button
          key={provider.id}
          type="button"
          onClick={() => onSelect(provider.id)}
          className="flex flex-col items-center gap-3 p-6 rounded-xl border-2 border-gray-200 bg-white
            hover:border-indigo-400 hover:shadow-md focus:outline-none focus:ring-2 focus:ring-indigo-500
            focus:border-indigo-500 transition-all dark:bg-gray-800 dark:border-gray-700
            dark:hover:border-indigo-500"
        >
          {provider.icon}
          <span className="font-semibold text-gray-900 dark:text-gray-100">{provider.name}</span>
          <span className="text-xs text-gray-500 dark:text-gray-400 text-center">
            {provider.description}
          </span>
        </button>
      ))}
    </div>
  );
}
