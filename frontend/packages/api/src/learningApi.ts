import { api } from './client.js';

export interface FeedbackAction {
  type: 'reclassify' | 'move_to_group';
  from?: string;
  to?: string;
  group_id?: string;
}

export interface FeedbackPayload {
  email_id: string;
  action: FeedbackAction;
}

export async function submitFeedback(payload: FeedbackPayload): Promise<void> {
  await api.post('learning/feedback', { json: payload });
}
