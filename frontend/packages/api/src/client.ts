import ky, { type AfterResponseState } from 'ky';

function notifyAuthenticationFailure({ response }: AfterResponseState): void {
  if (response.status === 401 && typeof window !== 'undefined') {
    window.dispatchEvent(new Event('emailibrium:unauthorized'));
  }
}

export const api = ky.create({
  prefix: '/api/v1',
  timeout: 30_000,
  credentials: 'same-origin',
  hooks: {
    afterResponse: [notifyAuthenticationFailure],
  },
});
