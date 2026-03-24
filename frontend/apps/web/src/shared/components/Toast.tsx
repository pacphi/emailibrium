import { useEffect, useRef } from 'react';
import { AlertCircle, CheckCircle, Info, X, AlertTriangle } from 'lucide-react';
import { type ToastItem, type ToastType, useToastStore } from '@/shared/stores/toastStore';

const ICON_MAP: Record<ToastType, typeof CheckCircle> = {
  success: CheckCircle,
  error: AlertCircle,
  warning: AlertTriangle,
  info: Info,
};

const COLOR_MAP: Record<ToastType, string> = {
  success: 'bg-green-50 border-green-300 text-green-800',
  error: 'bg-red-50 border-red-300 text-red-800',
  warning: 'bg-yellow-50 border-yellow-300 text-yellow-800',
  info: 'bg-blue-50 border-blue-300 text-blue-800',
};

const ICON_COLOR_MAP: Record<ToastType, string> = {
  success: 'text-green-500',
  error: 'text-red-500',
  warning: 'text-yellow-500',
  info: 'text-blue-500',
};

function ToastNotification({ toast }: { toast: ToastItem }) {
  const removeToast = useToastStore((state) => state.removeToast);
  const progressRef = useRef<HTMLDivElement>(null);
  const Icon = ICON_MAP[toast.type];

  useEffect(() => {
    const el = progressRef.current;
    if (!el || toast.duration <= 0) return;

    el.style.transition = 'none';
    el.style.width = '100%';

    // Force reflow then animate
    void el.offsetWidth;
    el.style.transition = `width ${toast.duration}ms linear`;
    el.style.width = '0%';
  }, [toast.duration]);

  return (
    <div
      role="alert"
      className={`relative overflow-hidden rounded-lg border px-4 py-3 shadow-lg ${COLOR_MAP[toast.type]}`}
      style={{ minWidth: 300, maxWidth: 420 }}
    >
      <div className="flex items-start gap-3">
        <Icon className={`mt-0.5 h-5 w-5 flex-shrink-0 ${ICON_COLOR_MAP[toast.type]}`} />
        <p className="flex-1 text-sm font-medium">{toast.message}</p>
        <button
          type="button"
          onClick={() => removeToast(toast.id)}
          className="flex-shrink-0 rounded p-0.5 opacity-60 transition-opacity hover:opacity-100"
          aria-label="Dismiss notification"
        >
          <X className="h-4 w-4" />
        </button>
      </div>
      {toast.duration > 0 && (
        <div className="absolute bottom-0 left-0 right-0 h-0.5 bg-current opacity-20">
          <div ref={progressRef} className="h-full bg-current opacity-60" />
        </div>
      )}
    </div>
  );
}

/**
 * Renders the stack of active toast notifications.
 * Mount this once near the root of your application.
 */
export function ToastContainer() {
  const toasts = useToastStore((state) => state.toasts);

  if (toasts.length === 0) return null;

  return (
    <div
      aria-live="polite"
      className="pointer-events-none fixed bottom-4 right-4 z-50 flex flex-col-reverse gap-2"
    >
      {toasts.map((toast) => (
        <div key={toast.id} className="pointer-events-auto animate-slide-up">
          <ToastNotification toast={toast} />
        </div>
      ))}
    </div>
  );
}
