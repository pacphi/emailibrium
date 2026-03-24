import { useState, useRef, useCallback } from 'react';
import { Upload, X, FileText, Image, Film, Music, File } from 'lucide-react';

export interface UploadedFile {
  id: string;
  file: File;
  progress: number;
}

interface AttachmentUploadProps {
  files: UploadedFile[];
  onAdd: (newFiles: File[]) => void;
  onRemove: (fileId: string) => void;
}

function getFileIcon(type: string) {
  if (type.startsWith('image/')) return Image;
  if (type.startsWith('video/')) return Film;
  if (type.startsWith('audio/')) return Music;
  if (type.includes('pdf') || type.includes('text')) return FileText;
  return File;
}

function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function AttachmentUpload({ files, onAdd, onRemove }: AttachmentUploadProps) {
  const [isDragOver, setIsDragOver] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  const handleDrop = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault();
      setIsDragOver(false);
      const droppedFiles = Array.from(e.dataTransfer.files);
      if (droppedFiles.length > 0) {
        onAdd(droppedFiles);
      }
    },
    [onAdd],
  );

  const handleFileInput = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      const selected = e.target.files ? Array.from(e.target.files) : [];
      if (selected.length > 0) {
        onAdd(selected);
      }
      // Reset input so same file can be selected again
      if (inputRef.current) inputRef.current.value = '';
    },
    [onAdd],
  );

  return (
    <div>
      {/* Drop zone */}
      <div
        onDragOver={(e) => {
          e.preventDefault();
          setIsDragOver(true);
        }}
        onDragLeave={() => setIsDragOver(false)}
        onDrop={handleDrop}
        onClick={() => inputRef.current?.click()}
        onKeyDown={(e) => {
          if (e.key === 'Enter' || e.key === ' ') {
            e.preventDefault();
            inputRef.current?.click();
          }
        }}
        role="button"
        tabIndex={0}
        aria-label="Drop files or click to attach"
        className={`
          cursor-pointer rounded-lg border-2 border-dashed p-4 text-center transition-colors
          ${
            isDragOver
              ? 'border-indigo-400 bg-indigo-50 dark:border-indigo-500 dark:bg-indigo-900/20'
              : 'border-gray-300 hover:border-gray-400 dark:border-gray-600 dark:hover:border-gray-500'
          }
        `}
      >
        <Upload className="mx-auto h-6 w-6 text-gray-400" aria-hidden="true" />
        <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">
          Drop files here or click to attach
        </p>
        <input
          ref={inputRef}
          type="file"
          multiple
          onChange={handleFileInput}
          className="hidden"
          aria-hidden="true"
        />
      </div>

      {/* File list */}
      {files.length > 0 && (
        <ul className="mt-3 space-y-2" aria-label="Attached files">
          {files.map((f) => {
            const Icon = getFileIcon(f.file.type);
            return (
              <li
                key={f.id}
                className="flex items-center gap-3 rounded-md border border-gray-200 bg-gray-50 px-3 py-2 dark:border-gray-600 dark:bg-gray-700"
              >
                <Icon className="h-4 w-4 shrink-0 text-gray-400" aria-hidden="true" />
                <div className="min-w-0 flex-1">
                  <p className="truncate text-sm font-medium text-gray-700 dark:text-gray-200">
                    {f.file.name}
                  </p>
                  <p className="text-xs text-gray-400">{formatFileSize(f.file.size)}</p>
                  {f.progress < 100 && (
                    <div className="mt-1 h-1 w-full overflow-hidden rounded-full bg-gray-200 dark:bg-gray-600">
                      <div
                        className="h-full rounded-full bg-indigo-500 transition-all"
                        style={{ width: `${f.progress}%` }}
                        role="progressbar"
                        aria-valuenow={f.progress}
                        aria-valuemin={0}
                        aria-valuemax={100}
                        aria-label={`Upload progress for ${f.file.name}`}
                      />
                    </div>
                  )}
                </div>
                <button
                  type="button"
                  onClick={() => onRemove(f.id)}
                  className="shrink-0 rounded p-1 text-gray-400 hover:bg-gray-200 hover:text-gray-600 dark:hover:bg-gray-600 dark:hover:text-gray-300"
                  aria-label={`Remove ${f.file.name}`}
                >
                  <X className="h-4 w-4" />
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
