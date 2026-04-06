export interface ChatSession {
  id: string;
  title: string;
  createdAt: string;
  updatedAt: string;
  messageCount: number;
}

export interface ChatRequest {
  message: string;
  sessionId?: string;
  history?: Array<{ role: 'user' | 'assistant'; content: string }>;
}

export interface ChatResponse {
  message: string;
  sessionId: string;
  suggestions?: Array<{
    rule: {
      id: string;
      name: string;
      conditions: Array<{ field: string; operator: string; value: string }>;
      actions: Array<{ type: string; value?: string }>;
      isActive: boolean;
      matchCount: number;
      accuracy: number;
      createdAt: string;
    };
    reason: string;
    estimatedMatches: number;
  }>;
}

export interface ChatStreamChunk {
  type: 'token' | 'suggestion' | 'done' | 'error';
  content?: string;
  sessionId?: string;
  suggestions?: ChatResponse['suggestions'];
  error?: string;
}

export interface ToolCallEvent {
  id: string;
  name: string;
  arguments: Record<string, unknown>;
}

export interface ToolResultEvent {
  toolCallId: string;
  content: string;
  isError: boolean;
}

export interface ConfirmationEvent {
  confirmationId: string;
  toolName: string;
  toolArgs: Record<string, unknown>;
  description: string;
}

export type ChatStreamEventType =
  | 'token'
  | 'done'
  | 'error'
  | 'tool_call'
  | 'tool_result'
  | 'confirmation';

export interface ChatStreamEvent {
  type: ChatStreamEventType;
  content?: string;
  sessionId?: string;
  toolCall?: ToolCallEvent;
  toolResult?: ToolResultEvent;
  confirmation?: ConfirmationEvent;
}
