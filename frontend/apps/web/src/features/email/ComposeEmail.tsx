import { useState, useCallback } from 'react';
import { X, Send, Save } from 'lucide-react';
import { nanoid } from 'nanoid';
import ReactMarkdown from 'react-markdown';
import { AttachmentUpload } from './AttachmentUpload';
import type { UploadedFile } from './AttachmentUpload';
import { useSendEmail } from './hooks/useEmails';

interface ComposeEmailProps {
  isOpen: boolean;
  onClose: () => void;
  accounts: { id: string; emailAddress: string; provider: string }[];
  /** Pre-fill for reply context */
  prefill?: {
    to?: string;
    cc?: string;
    subject?: string;
    accountId?: string;
    quotedBody?: string;
  };
}

export function ComposeEmail({ isOpen, onClose, accounts, prefill }: ComposeEmailProps) {
  const [fromAccountId, setFromAccountId] = useState(prefill?.accountId ?? accounts[0]?.id ?? '');
  const [to, setTo] = useState(prefill?.to ?? '');
  const [cc, setCc] = useState(prefill?.cc ?? '');
  const [bcc, setBcc] = useState('');
  const [subject, setSubject] = useState(prefill?.subject ?? '');
  const [body, setBody] = useState(prefill?.quotedBody ?? '');
  const [showPreview, setShowPreview] = useState(false);
  const [showCcBcc, setShowCcBcc] = useState(!!prefill?.cc);
  const [files, setFiles] = useState<UploadedFile[]>([]);

  const sendMutation = useSendEmail();

  const handleAddFiles = useCallback((newFiles: File[]) => {
    setFiles((prev) => [
      ...prev,
      ...newFiles.map((f) => ({ id: nanoid(), file: f, progress: 100 })),
    ]);
  }, []);

  const handleRemoveFile = useCallback((fileId: string) => {
    setFiles((prev) => prev.filter((f) => f.id !== fileId));
  }, []);

  function handleSend() {
    if (!to.trim() || !fromAccountId) return;
    sendMutation.mutate(
      {
        accountId: fromAccountId,
        to: to.trim(),
        cc: cc.trim() || undefined,
        bcc: bcc.trim() || undefined,
        subject: subject.trim(),
        bodyText: body,
      },
      {
        onSuccess: () => {
          onClose();
        },
      },
    );
  }

  function handleDiscard() {
    onClose();
  }

  function handleKeyDown(e: React.KeyboardEvent) {
    if (e.key === 'Escape') {
      onClose();
    }
  }

  if (!isOpen) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40"
      onKeyDown={handleKeyDown}
      role="dialog"
      aria-modal="true"
      aria-label="Compose email"
    >
      <div className="mx-4 flex max-h-[90vh] w-full max-w-2xl flex-col rounded-xl border border-gray-200 bg-white shadow-2xl dark:border-gray-700 dark:bg-gray-800">
        {/* Header */}
        <div className="flex items-center justify-between border-b border-gray-200 px-4 py-3 dark:border-gray-700">
          <h2 className="text-base font-semibold text-gray-900 dark:text-white">New Message</h2>
          <button
            type="button"
            onClick={onClose}
            className="rounded p-1 text-gray-400 hover:bg-gray-100 hover:text-gray-600 dark:hover:bg-gray-700 dark:hover:text-gray-300"
            aria-label="Close composer"
          >
            <X className="h-5 w-5" />
          </button>
        </div>

        {/* Form */}
        <div className="flex-1 overflow-y-auto p-4">
          <div className="space-y-3">
            {/* From */}
            <div className="flex items-center gap-2">
              <label
                htmlFor="compose-from"
                className="w-12 shrink-0 text-sm text-gray-500 dark:text-gray-400"
              >
                From
              </label>
              <select
                id="compose-from"
                value={fromAccountId}
                onChange={(e) => setFromAccountId(e.target.value)}
                className="flex-1 rounded-md border border-gray-200 bg-transparent px-2 py-1.5 text-sm text-gray-900 outline-none focus:border-indigo-400 dark:border-gray-600 dark:text-white"
              >
                {accounts.map((acc) => (
                  <option key={acc.id} value={acc.id}>
                    {acc.emailAddress} ({acc.provider})
                  </option>
                ))}
              </select>
            </div>

            {/* To */}
            <div className="flex items-center gap-2">
              <label
                htmlFor="compose-to"
                className="w-12 shrink-0 text-sm text-gray-500 dark:text-gray-400"
              >
                To
              </label>
              <input
                id="compose-to"
                type="text"
                value={to}
                onChange={(e) => setTo(e.target.value)}
                placeholder="recipient@example.com"
                className="flex-1 rounded-md border border-gray-200 bg-transparent px-2 py-1.5 text-sm text-gray-900 outline-none focus:border-indigo-400 dark:border-gray-600 dark:text-white"
              />
              {!showCcBcc && (
                <button
                  type="button"
                  onClick={() => setShowCcBcc(true)}
                  className="text-xs text-indigo-600 hover:underline dark:text-indigo-400"
                >
                  Cc/Bcc
                </button>
              )}
            </div>

            {/* Cc / Bcc */}
            {showCcBcc && (
              <>
                <div className="flex items-center gap-2">
                  <label
                    htmlFor="compose-cc"
                    className="w-12 shrink-0 text-sm text-gray-500 dark:text-gray-400"
                  >
                    Cc
                  </label>
                  <input
                    id="compose-cc"
                    type="text"
                    value={cc}
                    onChange={(e) => setCc(e.target.value)}
                    className="flex-1 rounded-md border border-gray-200 bg-transparent px-2 py-1.5 text-sm text-gray-900 outline-none focus:border-indigo-400 dark:border-gray-600 dark:text-white"
                  />
                </div>
                <div className="flex items-center gap-2">
                  <label
                    htmlFor="compose-bcc"
                    className="w-12 shrink-0 text-sm text-gray-500 dark:text-gray-400"
                  >
                    Bcc
                  </label>
                  <input
                    id="compose-bcc"
                    type="text"
                    value={bcc}
                    onChange={(e) => setBcc(e.target.value)}
                    className="flex-1 rounded-md border border-gray-200 bg-transparent px-2 py-1.5 text-sm text-gray-900 outline-none focus:border-indigo-400 dark:border-gray-600 dark:text-white"
                  />
                </div>
              </>
            )}

            {/* Subject */}
            <div className="flex items-center gap-2">
              <label
                htmlFor="compose-subject"
                className="w-12 shrink-0 text-sm text-gray-500 dark:text-gray-400"
              >
                Subj
              </label>
              <input
                id="compose-subject"
                type="text"
                value={subject}
                onChange={(e) => setSubject(e.target.value)}
                placeholder="Subject"
                className="flex-1 rounded-md border border-gray-200 bg-transparent px-2 py-1.5 text-sm text-gray-900 outline-none focus:border-indigo-400 dark:border-gray-600 dark:text-white"
              />
            </div>

            {/* Body + Preview toggle */}
            <div>
              <div className="mb-1 flex items-center justify-end gap-2">
                <button
                  type="button"
                  onClick={() => setShowPreview(false)}
                  className={`text-xs ${!showPreview ? 'font-semibold text-indigo-600' : 'text-gray-400 hover:text-gray-600'}`}
                >
                  Write
                </button>
                <button
                  type="button"
                  onClick={() => setShowPreview(true)}
                  className={`text-xs ${showPreview ? 'font-semibold text-indigo-600' : 'text-gray-400 hover:text-gray-600'}`}
                >
                  Preview
                </button>
              </div>
              {showPreview ? (
                <div className="min-h-[160px] rounded-md border border-gray-200 bg-gray-50 p-3 dark:border-gray-600 dark:bg-gray-700">
                  <div className="prose prose-sm max-w-none dark:prose-invert">
                    <ReactMarkdown>{body || '*No content*'}</ReactMarkdown>
                  </div>
                </div>
              ) : (
                <textarea
                  value={body}
                  onChange={(e) => setBody(e.target.value)}
                  rows={8}
                  placeholder="Compose your message (Markdown supported)..."
                  className="w-full resize-y rounded-md border border-gray-200 bg-white px-3 py-2 text-sm text-gray-900 outline-none focus:border-indigo-400 focus:ring-1 focus:ring-indigo-400 dark:border-gray-600 dark:bg-gray-700 dark:text-white"
                  aria-label="Email body"
                />
              )}
            </div>

            {/* Attachments */}
            <AttachmentUpload files={files} onAdd={handleAddFiles} onRemove={handleRemoveFile} />
          </div>
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between border-t border-gray-200 px-4 py-3 dark:border-gray-700">
          <button
            type="button"
            onClick={handleDiscard}
            className="text-sm text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"
          >
            Discard
          </button>
          <div className="flex items-center gap-2">
            <button
              type="button"
              className="flex items-center gap-1 rounded-md border border-gray-200 px-3 py-1.5 text-sm text-gray-600 transition-colors hover:bg-gray-50 dark:border-gray-600 dark:text-gray-300 dark:hover:bg-gray-700"
              aria-label="Save draft"
            >
              <Save className="h-4 w-4" aria-hidden="true" />
              Draft
            </button>
            <button
              type="button"
              onClick={handleSend}
              disabled={sendMutation.isPending || !to.trim()}
              className="flex items-center gap-1.5 rounded-md bg-indigo-600 px-4 py-1.5 text-sm font-medium text-white transition-colors hover:bg-indigo-700 disabled:cursor-not-allowed disabled:opacity-50"
              aria-label="Send email"
            >
              <Send className="h-4 w-4" aria-hidden="true" />
              {sendMutation.isPending ? 'Sending...' : 'Send'}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
