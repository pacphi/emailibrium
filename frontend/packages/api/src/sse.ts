export interface SSEStream<T> {
  subscribe: (handler: (data: T) => void) => void;
  close: () => void;
}

export function createSSEStream<T>(
  url: string,
  options?: { onError?: (e: Event) => void },
): SSEStream<T> {
  const source = new EventSource(url, { withCredentials: true });

  return {
    subscribe(handler: (data: T) => void) {
      source.onmessage = (event: MessageEvent) => {
        try {
          const data = JSON.parse(event.data) as T;
          handler(data);
        } catch {
          // Silently skip malformed messages
        }
      };

      source.onerror = (event: Event) => {
        options?.onError?.(event);
      };
    },

    close() {
      source.close();
    },
  };
}
