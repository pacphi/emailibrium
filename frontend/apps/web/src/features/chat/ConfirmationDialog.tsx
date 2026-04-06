import React from 'react';

interface ConfirmationDialogProps {
  toolName: string;
  description: string;
  onApprove: () => void;
  onReject: () => void;
}

export function ConfirmationDialog({
  toolName,
  description,
  onApprove,
  onReject,
}: ConfirmationDialogProps) {
  return (
    <div className="border border-yellow-300 bg-yellow-50 rounded-lg p-4 my-2">
      <div className="font-medium text-yellow-800 mb-2">Confirmation Required: {toolName}</div>
      <p className="text-sm text-yellow-700 mb-3">{description}</p>
      <div className="flex gap-2">
        <button
          onClick={onApprove}
          className="px-3 py-1 text-sm bg-blue-600 text-white rounded hover:bg-blue-700"
        >
          Approve
        </button>
        <button
          onClick={onReject}
          className="px-3 py-1 text-sm bg-gray-200 text-gray-700 rounded hover:bg-gray-300"
        >
          Cancel
        </button>
      </div>
    </div>
  );
}
