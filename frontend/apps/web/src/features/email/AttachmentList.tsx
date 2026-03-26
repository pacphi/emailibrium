import { useQuery } from '@tanstack/react-query';
import {
  Paperclip,
  FileText,
  Image,
  File,
  FileSpreadsheet,
  Download,
  FileArchive,
  FileCode,
  FileAudio,
  FileVideo,
} from 'lucide-react';
import { getAttachments, getAttachmentDownloadUrl, getAttachmentsZipUrl } from '@emailibrium/api';
import type { Attachment } from '@emailibrium/types';

function getFileIcon(contentType: string) {
  if (contentType.startsWith('image/')) return Image;
  if (contentType.startsWith('audio/')) return FileAudio;
  if (contentType.startsWith('video/')) return FileVideo;
  if (contentType === 'application/pdf') return FileText;
  if (contentType.includes('spreadsheet') || contentType.includes('excel')) return FileSpreadsheet;
  if (
    contentType.includes('zip') ||
    contentType.includes('compressed') ||
    contentType.includes('archive')
  )
    return FileArchive;
  if (
    contentType.includes('javascript') ||
    contentType.includes('json') ||
    contentType.includes('xml') ||
    contentType.includes('html')
  )
    return FileCode;
  if (contentType.startsWith('text/')) return FileText;
  return File;
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function AttachmentChip({ attachment, emailId }: { attachment: Attachment; emailId: string }) {
  const Icon = getFileIcon(attachment.contentType);
  const downloadUrl = getAttachmentDownloadUrl(emailId, attachment.id);

  return (
    <a
      href={downloadUrl}
      download={attachment.filename}
      className="inline-flex items-center gap-2 rounded-lg border border-gray-200 px-3 py-2 text-sm hover:bg-gray-50 dark:border-gray-700 dark:hover:bg-gray-800 transition-colors"
    >
      <Icon className="h-4 w-4 shrink-0 text-gray-500 dark:text-gray-400" />
      <span className="max-w-[180px] truncate text-gray-700 dark:text-gray-300">
        {attachment.filename}
      </span>
      <span className="shrink-0 text-xs text-gray-400 dark:text-gray-500">
        {formatSize(attachment.sizeBytes)}
      </span>
    </a>
  );
}

function ImageAttachmentPreview({
  attachment,
  emailId,
}: {
  attachment: Attachment;
  emailId: string;
}) {
  const downloadUrl = getAttachmentDownloadUrl(emailId, attachment.id);

  return (
    <a
      href={downloadUrl}
      target="_blank"
      rel="noopener noreferrer"
      className="group relative overflow-hidden rounded-lg border border-gray-200 dark:border-gray-700"
    >
      <img
        src={downloadUrl}
        alt={attachment.filename}
        loading="lazy"
        className="h-24 w-auto object-cover"
      />
      <div className="absolute inset-x-0 bottom-0 bg-black/60 px-2 py-1 text-xs text-white opacity-0 group-hover:opacity-100 transition-opacity">
        {attachment.filename}
      </div>
    </a>
  );
}

function DownloadAllButton({ emailId, count }: { emailId: string; count: number }) {
  const zipUrl = getAttachmentsZipUrl(emailId);

  return (
    <a
      href={zipUrl}
      download="attachments.zip"
      className="inline-flex items-center gap-1 text-sm text-indigo-600 hover:text-indigo-700 dark:text-indigo-400 dark:hover:text-indigo-300"
    >
      <Download className="h-4 w-4" />
      Download all ({count})
    </a>
  );
}

export function AttachmentList({ emailId }: { emailId: string }) {
  const { data: attachments, isLoading } = useQuery({
    queryKey: ['attachments', emailId],
    queryFn: () => getAttachments(emailId),
    staleTime: 60_000,
  });

  if (isLoading || !attachments || attachments.length === 0) return null;

  const fileAttachments = attachments.filter((a) => !a.isInline);
  const imageAttachments = fileAttachments.filter((a) => a.contentType.startsWith('image/'));
  const otherAttachments = fileAttachments.filter((a) => !a.contentType.startsWith('image/'));

  if (fileAttachments.length === 0) return null;

  return (
    <div className="mt-3 space-y-3">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-1 text-xs font-medium text-gray-500 dark:text-gray-400">
          <Paperclip className="h-3.5 w-3.5" />
          {fileAttachments.length} attachment{fileAttachments.length !== 1 ? 's' : ''}
        </div>
        {fileAttachments.length >= 2 && (
          <DownloadAllButton emailId={emailId} count={fileAttachments.length} />
        )}
      </div>

      {imageAttachments.length > 0 && (
        <div className="flex flex-wrap gap-2">
          {imageAttachments.map((att) => (
            <ImageAttachmentPreview key={att.id} attachment={att} emailId={emailId} />
          ))}
        </div>
      )}

      {otherAttachments.length > 0 && (
        <div className="flex flex-wrap gap-2">
          {otherAttachments.map((att) => (
            <AttachmentChip key={att.id} attachment={att} emailId={emailId} />
          ))}
        </div>
      )}
    </div>
  );
}
