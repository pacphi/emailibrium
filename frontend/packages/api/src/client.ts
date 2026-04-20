import ky, { type BeforeRequestState } from 'ky';

function addAuthHeader({ request }: BeforeRequestState): void {
  const token = localStorage.getItem('auth_token');
  if (token) {
    request.headers.set('Authorization', `Bearer ${token}`);
  }
}

export const api = ky.create({
  prefix: '/api/v1',
  timeout: 30_000,
  hooks: {
    beforeRequest: [addAuthHeader],
  },
});
