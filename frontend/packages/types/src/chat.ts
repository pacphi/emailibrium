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
