import { useState } from 'react';
import { useForm } from 'react-hook-form';
import { z } from 'zod';
import type { EmailAccount } from '@emailibrium/types';
import { api } from '@emailibrium/api';

const imapSchema = z.object({
  email: z.string().email('Enter a valid email address'),
  password: z.string().min(1, 'Password or app password is required'),
  imapServer: z.string().min(1, 'IMAP server is required'),
  imapPort: z.coerce.number().int().min(1).max(65535),
  smtpServer: z.string().min(1, 'SMTP server is required'),
  smtpPort: z.coerce.number().int().min(1).max(65535),
  encryption: z.enum(['ssl', 'tls', 'none']),
});

type ImapFormData = z.infer<typeof imapSchema>;

interface ImapPreset {
  name: string;
  imapServer: string;
  imapPort: number;
  smtpServer: string;
  smtpPort: number;
  encryption: 'ssl' | 'tls' | 'none';
}

const PRESETS: ImapPreset[] = [
  {
    name: 'Yahoo Mail',
    imapServer: 'imap.mail.yahoo.com',
    imapPort: 993,
    smtpServer: 'smtp.mail.yahoo.com',
    smtpPort: 465,
    encryption: 'ssl',
  },
  {
    name: 'iCloud',
    imapServer: 'imap.mail.me.com',
    imapPort: 993,
    smtpServer: 'smtp.mail.me.com',
    smtpPort: 587,
    encryption: 'tls',
  },
  {
    name: 'Fastmail',
    imapServer: 'imap.fastmail.com',
    imapPort: 993,
    smtpServer: 'smtp.fastmail.com',
    smtpPort: 465,
    encryption: 'ssl',
  },
  {
    name: 'Zoho Mail',
    imapServer: 'imap.zoho.com',
    imapPort: 993,
    smtpServer: 'smtp.zoho.com',
    smtpPort: 465,
    encryption: 'ssl',
  },
  {
    name: 'ProtonMail Bridge',
    imapServer: '127.0.0.1',
    imapPort: 1143,
    smtpServer: '127.0.0.1',
    smtpPort: 1025,
    encryption: 'tls',
  },
];

interface ImapConnectProps {
  onBack: () => void;
  onConnected: (account: EmailAccount) => void;
}

export function ImapConnect({ onBack, onConnected }: ImapConnectProps) {
  const [testStatus, setTestStatus] = useState<'idle' | 'testing' | 'success' | 'error'>('idle');
  const [testError, setTestError] = useState<string | null>(null);

  const {
    register,
    handleSubmit,
    setValue,
    formState: { errors, isSubmitting },
  } = useForm<ImapFormData>({
    defaultValues: {
      email: '',
      password: '',
      imapServer: '',
      imapPort: 993,
      smtpServer: '',
      smtpPort: 587,
      encryption: 'ssl',
    },
  });

  function applyPreset(preset: ImapPreset) {
    setValue('imapServer', preset.imapServer);
    setValue('imapPort', preset.imapPort);
    setValue('smtpServer', preset.smtpServer);
    setValue('smtpPort', preset.smtpPort);
    setValue('encryption', preset.encryption);
  }

  async function handleTestConnection(data: ImapFormData) {
    setTestStatus('testing');
    setTestError(null);
    try {
      await api.post('auth/imap/test', { json: data }).json();
      setTestStatus('success');
    } catch (err) {
      setTestStatus('error');
      setTestError(err instanceof Error ? err.message : 'Connection test failed');
    }
  }

  async function onSubmit(data: ImapFormData) {
    try {
      const account = await api.post('auth/imap/connect', { json: data }).json<EmailAccount>();
      onConnected(account);
    } catch (err) {
      setTestStatus('error');
      setTestError(err instanceof Error ? err.message : 'Failed to connect account');
    }
  }

  return (
    <div className="max-w-lg mx-auto space-y-6">
      <button
        type="button"
        onClick={onBack}
        className="flex items-center gap-1 text-sm text-gray-500 hover:text-gray-700 dark:text-gray-400
          dark:hover:text-gray-200 transition-colors"
      >
        <svg viewBox="0 0 20 20" fill="currentColor" className="w-4 h-4" aria-hidden="true">
          <path
            fillRule="evenodd"
            d="M17 10a.75.75 0 01-.75.75H5.612l4.158 3.96a.75.75 0 11-1.04 1.08l-5.5-5.25a.75.75 0 010-1.08l5.5-5.25a.75.75 0 111.04 1.08L5.612 9.25H16.25A.75.75 0 0117 10z"
            clipRule="evenodd"
          />
        </svg>
        Back to providers
      </button>

      <div className="text-center space-y-1">
        <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">Connect via IMAP</h3>
        <p className="text-sm text-gray-500 dark:text-gray-400">
          Enter your mail server details or select a preset below.
        </p>
      </div>

      {/* Preset selector */}
      <div>
        <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
          Provider Preset
        </label>
        <select
          onChange={(e) => {
            const preset = PRESETS.find((p) => p.name === e.target.value);
            if (preset) applyPreset(preset);
          }}
          defaultValue=""
          className="w-full rounded-lg border border-gray-300 bg-white px-3 py-2 text-sm
            focus:border-indigo-500 focus:ring-1 focus:ring-indigo-500
            dark:bg-gray-800 dark:border-gray-600 dark:text-gray-200"
        >
          <option value="" disabled>
            Select a provider...
          </option>
          {PRESETS.map((preset) => (
            <option key={preset.name} value={preset.name}>
              {preset.name}
            </option>
          ))}
        </select>
      </div>

      <form onSubmit={handleSubmit(onSubmit)} className="space-y-4">
        {/* Email */}
        <div>
          <label
            htmlFor="imap-email"
            className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1"
          >
            Email Address
          </label>
          <input
            id="imap-email"
            type="email"
            {...register('email', { required: true })}
            className="w-full rounded-lg border border-gray-300 px-3 py-2 text-sm
              focus:border-indigo-500 focus:ring-1 focus:ring-indigo-500
              dark:bg-gray-800 dark:border-gray-600 dark:text-gray-200"
            placeholder="you@example.com"
          />
          {errors.email && <p className="mt-1 text-xs text-red-600">{errors.email.message}</p>}
        </div>

        {/* Password */}
        <div>
          <label
            htmlFor="imap-password"
            className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1"
          >
            Password / App Password
          </label>
          <input
            id="imap-password"
            type="password"
            {...register('password', { required: true })}
            className="w-full rounded-lg border border-gray-300 px-3 py-2 text-sm
              focus:border-indigo-500 focus:ring-1 focus:ring-indigo-500
              dark:bg-gray-800 dark:border-gray-600 dark:text-gray-200"
            placeholder="App-specific password recommended"
          />
          {errors.password && (
            <p className="mt-1 text-xs text-red-600">{errors.password.message}</p>
          )}
        </div>

        {/* IMAP Server + Port */}
        <div className="grid grid-cols-3 gap-3">
          <div className="col-span-2">
            <label
              htmlFor="imap-server"
              className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1"
            >
              IMAP Server
            </label>
            <input
              id="imap-server"
              type="text"
              {...register('imapServer', { required: true })}
              className="w-full rounded-lg border border-gray-300 px-3 py-2 text-sm
                focus:border-indigo-500 focus:ring-1 focus:ring-indigo-500
                dark:bg-gray-800 dark:border-gray-600 dark:text-gray-200"
              placeholder="imap.example.com"
            />
            {errors.imapServer && (
              <p className="mt-1 text-xs text-red-600">{errors.imapServer.message}</p>
            )}
          </div>
          <div>
            <label
              htmlFor="imap-port"
              className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1"
            >
              Port
            </label>
            <input
              id="imap-port"
              type="number"
              {...register('imapPort', { required: true, valueAsNumber: true })}
              className="w-full rounded-lg border border-gray-300 px-3 py-2 text-sm
                focus:border-indigo-500 focus:ring-1 focus:ring-indigo-500
                dark:bg-gray-800 dark:border-gray-600 dark:text-gray-200"
            />
            {errors.imapPort && (
              <p className="mt-1 text-xs text-red-600">{errors.imapPort.message}</p>
            )}
          </div>
        </div>

        {/* SMTP Server + Port */}
        <div className="grid grid-cols-3 gap-3">
          <div className="col-span-2">
            <label
              htmlFor="smtp-server"
              className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1"
            >
              SMTP Server
            </label>
            <input
              id="smtp-server"
              type="text"
              {...register('smtpServer', { required: true })}
              className="w-full rounded-lg border border-gray-300 px-3 py-2 text-sm
                focus:border-indigo-500 focus:ring-1 focus:ring-indigo-500
                dark:bg-gray-800 dark:border-gray-600 dark:text-gray-200"
              placeholder="smtp.example.com"
            />
            {errors.smtpServer && (
              <p className="mt-1 text-xs text-red-600">{errors.smtpServer.message}</p>
            )}
          </div>
          <div>
            <label
              htmlFor="smtp-port"
              className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1"
            >
              Port
            </label>
            <input
              id="smtp-port"
              type="number"
              {...register('smtpPort', { required: true, valueAsNumber: true })}
              className="w-full rounded-lg border border-gray-300 px-3 py-2 text-sm
                focus:border-indigo-500 focus:ring-1 focus:ring-indigo-500
                dark:bg-gray-800 dark:border-gray-600 dark:text-gray-200"
            />
            {errors.smtpPort && (
              <p className="mt-1 text-xs text-red-600">{errors.smtpPort.message}</p>
            )}
          </div>
        </div>

        {/* Encryption */}
        <div>
          <label
            htmlFor="imap-encryption"
            className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1"
          >
            Encryption
          </label>
          <select
            id="imap-encryption"
            {...register('encryption')}
            className="w-full rounded-lg border border-gray-300 bg-white px-3 py-2 text-sm
              focus:border-indigo-500 focus:ring-1 focus:ring-indigo-500
              dark:bg-gray-800 dark:border-gray-600 dark:text-gray-200"
          >
            <option value="ssl">SSL</option>
            <option value="tls">TLS (STARTTLS)</option>
            <option value="none">None</option>
          </select>
        </div>

        {/* Status messages */}
        {testStatus === 'success' && (
          <div className="flex items-center gap-2 rounded-lg bg-green-50 border border-green-200 px-3 py-2 text-sm text-green-700 dark:bg-green-900/20 dark:border-green-800 dark:text-green-400">
            <svg
              viewBox="0 0 20 20"
              fill="currentColor"
              className="w-4 h-4 shrink-0"
              aria-hidden="true"
            >
              <path
                fillRule="evenodd"
                d="M10 18a8 8 0 100-16 8 8 0 000 16zm3.857-9.809a.75.75 0 00-1.214-.882l-3.483 4.79-1.88-1.88a.75.75 0 10-1.06 1.061l2.5 2.5a.75.75 0 001.137-.089l4-5.5z"
                clipRule="evenodd"
              />
            </svg>
            Connection successful!
          </div>
        )}
        {testStatus === 'error' && testError && (
          <div className="flex items-center gap-2 rounded-lg bg-red-50 border border-red-200 px-3 py-2 text-sm text-red-700 dark:bg-red-900/20 dark:border-red-800 dark:text-red-400">
            <svg
              viewBox="0 0 20 20"
              fill="currentColor"
              className="w-4 h-4 shrink-0"
              aria-hidden="true"
            >
              <path
                fillRule="evenodd"
                d="M18 10a8 8 0 11-16 0 8 8 0 0116 0zm-8-5a.75.75 0 01.75.75v4.5a.75.75 0 01-1.5 0v-4.5A.75.75 0 0110 5zm0 10a1 1 0 100-2 1 1 0 000 2z"
                clipRule="evenodd"
              />
            </svg>
            {testError}
          </div>
        )}

        {/* Actions */}
        <div className="flex gap-3">
          <button
            type="button"
            onClick={handleSubmit(handleTestConnection)}
            disabled={testStatus === 'testing'}
            className="flex-1 px-4 py-2 rounded-lg border border-gray-300 text-gray-700 text-sm font-medium
              hover:bg-gray-50 disabled:opacity-60 disabled:cursor-not-allowed transition-colors
              dark:border-gray-600 dark:text-gray-300 dark:hover:bg-gray-700"
          >
            {testStatus === 'testing' ? 'Testing...' : 'Test Connection'}
          </button>
          <button
            type="submit"
            disabled={isSubmitting}
            className="flex-1 px-4 py-2 rounded-lg bg-indigo-600 text-white text-sm font-medium
              hover:bg-indigo-700 disabled:opacity-60 disabled:cursor-not-allowed transition-colors"
          >
            {isSubmitting ? 'Connecting...' : 'Connect Account'}
          </button>
        </div>
      </form>
    </div>
  );
}
