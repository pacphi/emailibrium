import { useRef, useEffect, useMemo, useState } from 'react';

interface EmailHtmlViewerProps {
  html: string;
}

/**
 * Renders untrusted HTML email content inside a sandboxed iframe.
 *
 * Security layers (ADR-019):
 * 1. sandbox attribute -- blocks JS execution, form submission, navigation
 * 2. CSP meta tag -- script-src 'none'; object-src 'none'
 * 3. referrerPolicy -- no-referrer prevents origin leakage
 */
export function EmailHtmlViewer({ html }: EmailHtmlViewerProps) {
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const [height, setHeight] = useState(200);

  const wrappedHtml = useMemo(() => {
    const csp = `<meta http-equiv="Content-Security-Policy" content="script-src 'none'; object-src 'none';">`;
    const base = `<base target="_blank">`;
    const meta = `<meta name="referrer" content="no-referrer">`;
    const style = `<style>
      body {
        margin: 0;
        padding: 8px;
        font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
        font-size: 14px;
        line-height: 1.5;
        color: #1f2937;
        word-wrap: break-word;
        overflow-wrap: break-word;
      }
      img { max-width: 100%; height: auto; }
      a { color: #4f46e5; }
      table { max-width: 100%; }
      pre { white-space: pre-wrap; overflow-x: auto; }
      blockquote {
        border-left: 3px solid #d1d5db;
        margin: 8px 0;
        padding: 4px 12px;
        color: #6b7280;
      }
    </style>`;
    return `<!DOCTYPE html><html><head>${csp}${base}${meta}${style}</head><body>${html}</body></html>`;
  }, [html]);

  useEffect(() => {
    const iframe = iframeRef.current;
    if (!iframe) return;

    const handleLoad = () => {
      try {
        const doc = iframe.contentDocument;
        if (doc) {
          const scrollHeight = doc.documentElement.scrollHeight;
          if (scrollHeight > 0) {
            setHeight(scrollHeight);
          }
        }
      } catch {
        // Cross-origin safety -- cannot access contentDocument
      }
    };

    iframe.addEventListener('load', handleLoad);
    return () => iframe.removeEventListener('load', handleLoad);
  }, [wrappedHtml]);

  return (
    <iframe
      ref={iframeRef}
      srcDoc={wrappedHtml}
      sandbox="allow-popups allow-popups-to-escape-sandbox allow-same-origin"
      referrerPolicy="no-referrer"
      title="Email content"
      className="w-full border-0"
      style={{ height, minHeight: 100 }}
    />
  );
}
